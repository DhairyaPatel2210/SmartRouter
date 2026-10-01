pub mod adapters;
pub mod brand;
pub mod commands;
pub mod db;
pub mod engine;
pub mod governor;
pub mod handoff;
pub mod installer;
pub mod library;
pub mod macos;
pub mod proc;
pub mod providers;
pub mod router;
pub mod settings;
pub mod telemetry;
pub mod types;
pub mod verify;
pub mod workspace;

use commands::AppState;
use engine::{Core, CoreDeps};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter as _, Manager, RunEvent, WindowEvent};
use telemetry::{Emitter, UiEvent};

pub const EVENT: &str = "orch://events";

static TRAY_STATUS: OnceLock<MenuItem<tauri::Wry>> = OnceLock::new();

/// Sends batched core events to the webview and keeps the menu bar summary current.
struct AppEmitter {
    app: AppHandle,
}

impl Emitter for AppEmitter {
    fn emit(&self, batch: Vec<UiEvent>) {
        let mut status = None;
        for e in &batch {
            if let UiEvent::Resources { snapshot } = e {
                status = Some(tray_text(snapshot));
            }
        }
        let _ = self.app.emit(EVENT, &batch);
        if let (Some(t), Some(item)) = (status, TRAY_STATUS.get()) {
            let _ = item.set_text(t);
        }
    }
}

fn tray_text(s: &telemetry::sampler::Snapshot) -> String {
    let free = format!("{:.1} GB free", s.sys.free_gb);
    if s.running.is_empty() {
        format!("Idle · {free}")
    } else {
        let mem = s.controlled_mb / 1024.0;
        format!("{} · {mem:.1} GB in use · {free} · ~${:.2}", s.running.join(", "), s.cost_usd)
    }
}

fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("ORCH_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(brand::DATA_DIR_NAME)
}

fn catalog_dir(app: &AppHandle) -> PathBuf {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("catalog"));
    match bundled {
        Some(d) if d.join("agents.json").exists() => d,
        _ => installer::catalog::dev_dir(),
    }
}

fn path_arg(args: &[String]) -> Option<String> {
    args.iter().skip(1).find(|a| !a.starts_with('-') && std::path::Path::new(a).is_dir()).cloned()
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(st) = app.try_state::<AppState>() {
        st.core.window_visible.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn build_tray(app: &AppHandle, core: Arc<Core>) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Idle", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", format!("Open {}", brand::PRODUCT_NAME), true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop everything", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", format!("Quit {}", brand::SHORT_NAME), true, Some("CmdOrCtrl+Q"))?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &sep1, &open, &stop, &sep2, &quit])?;
    let _ = TRAY_STATUS.set(status);
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .tooltip(brand::PRODUCT_NAME)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            "open" => show_main(app),
            "stop" => {
                let c = core.clone();
                tauri::async_runtime::spawn(async move {
                    c.stop_everything().await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            show_main(app);
            if let Some(p) = path_arg(&argv) {
                let _ = app.emit("orch://open-path", p);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let notify_handle = handle.clone();
            let core = Core::new(CoreDeps {
                data_dir: data_dir(),
                catalog_dir: catalog_dir(&handle),
                emitter: Arc::new(AppEmitter { app: handle.clone() }),
                demo_agents: std::env::var("ORCH_DEMO_AGENTS").is_ok() || cfg!(debug_assertions),
                notifier: Arc::new(move |title: &str, body: &str| {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = notify_handle.notification().builder().title(title).body(body).show();
                }),
            })?;
            let args: Vec<String> = std::env::args().collect();
            app.manage(AppState { core: core.clone(), pending_open: Mutex::new(path_arg(&args)) });

            if core.db.settings().menu_bar {
                if let Err(e) = build_tray(&handle, core.clone()) {
                    log::warn!("menu bar item unavailable: {e}");
                }
            }

            // Memory pressure: event-driven, costs nothing until the kernel signals.
            let pc = core.clone();
            macos::watch_pressure(move |level| {
                let c = pc.clone();
                tauri::async_runtime::spawn(async move { c.on_pressure(level).await });
            });

            // Detect agents after the window is up (PATH lookup + `--version` calls).
            let dc = core.clone();
            tauri::async_runtime::spawn(async move {
                dc.refresh_agents(true).await;
                dc.bus.send(UiEvent::Agents);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            let Some(st) = window.app_handle().try_state::<AppState>() else { return };
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // With the menu bar item on, closing hides the window; the app quits from the menu.
                    if st.core.db.settings().menu_bar && TRAY_STATUS.get().is_some() {
                        api.prevent_close();
                        let _ = window.hide();
                        st.core.window_visible.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                WindowEvent::Focused(true) => st.core.window_visible.store(true, std::sync::atomic::Ordering::SeqCst),
                WindowEvent::Resized(_) => {
                    let minimized = window.is_minimized().unwrap_or(false);
                    st.core.window_visible.store(!minimized, std::sync::atomic::Ordering::SeqCst);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::take_pending_open,
            commands::get_settings,
            commands::save_settings,
            commands::scan,
            commands::detect_runtimes,
            commands::open_workspace,
            commands::inspect_workspace,
            commands::recent_workspaces,
            commands::update_workspace,
            commands::remove_workspace,
            commands::init_git,
            commands::sample_project,
            commands::preflight,
            commands::start_run,
            commands::cancel_run,
            commands::pause_run,
            commands::set_run_mode,
            commands::reassign_step,
            commands::answer_approval,
            commands::pending_approvals,
            commands::get_run,
            commands::list_runs,
            commands::step_logs,
            commands::rollback_step,
            commands::accept_run,
            commands::export_run,
            commands::stop_everything,
            commands::library_list,
            commands::library_create,
            commands::library_save,
            commands::library_rename,
            commands::library_toggle,
            commands::library_references,
            commands::library_delete,
            commands::library_duplicate,
            commands::library_move,
            commands::library_preview,
            commands::library_sync,
            commands::library_import,
            commands::library_import_edit,
            commands::save_profile,
            commands::delete_profile,
            commands::agents,
            commands::agent_set,
            commands::install_agent,
            commands::uninstall_agent,
            commands::install_runtime,
            commands::cancel_job,
            commands::open_login,
            commands::local_models,
            commands::hardware_check,
            commands::model_suggestions,
            commands::ollama_status,
            commands::ollama_start,
            commands::ollama_stop,
            commands::pull_model,
            commands::cancel_pull,
            commands::delete_model,
            commands::unload_model,
            commands::set_model_folder,
            commands::move_models,
            commands::import_gguf,
            commands::providers_list,
            commands::connect_provider,
            commands::provider_models,
            commands::update_provider,
            commands::disconnect_provider,
            commands::connect_model,
            commands::smoke_test,
            commands::models_list,
            commands::update_model,
            commands::resources_view,
            commands::resource_snapshot,
            commands::why_slow,
            commands::metrics_since,
            commands::outcome_stats,
            commands::install_cli_command,
            commands::set_window_visible,
            commands::workflow_list,
            commands::workflow_save,
            commands::workflow_delete,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|app, event| match event {
        RunEvent::Reopen { .. } => show_main(app),
        RunEvent::Exit => {
            // Nothing keeps running after quit: unload what we loaded, stop what we started.
            if let Some(st) = app.try_state::<AppState>() {
                let core = st.core.clone();
                tauri::async_runtime::block_on(async {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), core.unload_app_models()).await;
                });
                core.shutdown();
            }
        }
        _ => {}
    });
}

/// `--fake-provider <port>`: serve the fake OpenAI-compatible API until killed.
pub fn run_fake_provider(port: u16) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    rt.block_on(async {
        let srv = providers::fake::start(port).await.expect("bind");
        println!("fake provider listening on {}", srv.base_url());
        std::future::pending::<()>().await;
    });
}
