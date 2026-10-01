//! Tauri commands: the only API the UI has. The UI renders and sends
//! commands; every process, file and model decision happens in the core.

use crate::adapters::Workspace as AdapterWs;
use crate::db::queries::{ModelRow, ProviderRow, RunRow, StepRow, WorkspaceRow};
use crate::engine::{self, ApprovalRequest, Assignment, Core, StartOpts};
use crate::governor::{self, service::OllamaStatus};
use crate::installer::{self, catalog::os_key, HardwareCheck, Suggestion};
use crate::library::{self, sync, Kind, Scope};
use crate::proc::CancelToken;
use crate::providers::{self, keychain, CloudModel};
use crate::settings::Settings;
use crate::telemetry::{sampler::Snapshot, LogLine, UiEvent};
use crate::types::*;
use crate::workspace::{self, WorkspaceInfo};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

pub type Res<T> = Result<T, String>;

/// Converts an error for the UI and logs it (so failures show in the terminal and app.log).
fn e<E: std::fmt::Display>(err: E) -> String {
    let s = format!("{err:#}");
    log::warn!("command error: {s}");
    s
}

/// Errors and warnings from the UI (render crashes, failed calls) go to the app log.
#[tauri::command]
pub fn ui_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("ui: {message}"),
        "warn" => log::warn!("ui: {message}"),
        _ => log::info!("ui: {message}"),
    }
}

pub struct AppState {
    pub core: Arc<Core>,
    /// Folder passed on the command line (`orch <path>`), taken once by the UI.
    pub pending_open: Mutex<Option<String>>,
}

type S<'a> = State<'a, AppState>;

// ---------------------------------------------------------------- bootstrap

#[derive(Serialize)]
pub struct Bootstrap {
    pub brand: serde_json::Value,
    pub settings: Settings,
    pub agents: Vec<engine::AgentInfo>,
    pub recent: Vec<WorkspaceRow>,
    pub active_runs: Vec<String>,
    pub approvals: Vec<ApprovalRequest>,
    pub data_dir: String,
    pub catalog: installer::Catalog,
    pub os: String,
    pub version: String,
}

#[tauri::command]
pub fn bootstrap(s: S) -> Res<Bootstrap> {
    let c = &s.core;
    Ok(Bootstrap {
        brand: serde_json::json!({
            "productName": crate::brand::PRODUCT_NAME, "shortName": crate::brand::SHORT_NAME,
            "handoffDirName": crate::brand::HANDOFF_DIR_NAME, "cliName": crate::brand::CLI_NAME,
        }),
        settings: c.db.settings(),
        agents: c.agent_infos(),
        recent: c.db.recent_workspaces(10).map_err(e)?,
        active_runs: c.active_runs().iter().map(|r| r.run_id.clone()).collect(),
        approvals: c.pending_approvals(),
        data_dir: c.data_dir.to_string_lossy().into_owned(),
        catalog: c.catalog.clone(),
        os: os_key().into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[tauri::command]
pub fn take_pending_open(s: S) -> Option<String> {
    // The UI calls this at the end of boot: the app is interactive.
    if std::env::var("ORCH_PERF").is_ok() {
        if let Some(t) = crate::STARTED.get() {
            println!("ORCH_READY {}", t.elapsed().as_millis());
        }
    }
    s.pending_open.lock().take()
}

#[tauri::command]
pub fn get_settings(s: S) -> Settings {
    s.core.db.settings()
}

#[tauri::command]
pub fn save_settings(s: S, settings: Settings) -> Res<Settings> {
    let mut settings = settings;
    settings.normalize();
    s.core.db.save_settings(&settings).map_err(e)?;
    s.core.logs.set_cap(settings.log_ring_lines);
    Ok(settings)
}

// ---------------------------------------------------------------- onboarding scan

#[derive(Serialize)]
pub struct ScanResult {
    pub agents: Vec<engine::AgentInfo>,
    pub hardware: HardwareCheck,
    pub ollama: OllamaStatus,
    pub local_models: Vec<LocalModel>,
    pub runtimes: Vec<providers::DetectedRuntime>,
    pub suggestions: Vec<Suggestion>,
    pub library_found: Vec<String>,
    pub elapsed_ms: u64,
}

/// The onboarding sweep: agents, runtimes, downloaded models, hardware and
/// existing agent config. Everything runs in parallel with short timeouts.
#[tauri::command]
pub async fn scan(s: S<'_>) -> Res<ScanResult> {
    let t = std::time::Instant::now();
    let c = s.core.clone();
    let settings = c.db.settings();
    let (agents, ollama, runtimes) = tokio::join!(c.refresh_agents(false), c.ollama.status(&settings), detect_runtimes_inner());
    let local = local_models_inner(&c).await;
    let hw = installer::hardware_check(&settings);
    let installed: Vec<String> = local.iter().map(|m| m.name.clone()).collect();
    let suggestions = installer::suggestions(&c.catalog, &hw, &installed);
    let mut library_found = vec![];
    if let Some(home) = dirs::home_dir() {
        for (p, what) in [
            (".claude/skills", "Claude skills"),
            (".claude/agents", "Claude agents"),
            (".cursor/rules", "Cursor rules"),
            (".codex/AGENTS.md", "Codex AGENTS.md"),
        ] {
            if home.join(p).exists() {
                library_found.push(what.to_string());
            }
        }
    }
    Ok(ScanResult {
        agents,
        hardware: hw,
        ollama,
        local_models: local,
        runtimes,
        suggestions,
        library_found,
        elapsed_ms: t.elapsed().as_millis() as u64,
    })
}

async fn detect_runtimes_inner() -> Vec<providers::DetectedRuntime> {
    let (a, b, c) = tokio::join!(
        providers::detect_openai_runtime(ProviderType::Lmstudio, "http://127.0.0.1:1234/v1"),
        providers::detect_openai_runtime(ProviderType::Llamacpp, "http://127.0.0.1:8080/v1"),
        providers::detect_openai_runtime(ProviderType::Mlx, "http://127.0.0.1:8081/v1"),
    );
    vec![a, b, c]
}

#[tauri::command]
pub async fn detect_runtimes() -> Vec<providers::DetectedRuntime> {
    detect_runtimes_inner().await
}

// ---------------------------------------------------------------- workspaces

#[derive(Serialize)]
pub struct OpenedWorkspace {
    pub row: WorkspaceRow,
    pub info: WorkspaceInfo,
}

#[tauri::command]
pub async fn open_workspace(s: S<'_>, path: String) -> Res<OpenedWorkspace> {
    let p = PathBuf::from(&path);
    let p = p.canonicalize().unwrap_or(p);
    if !p.is_dir() {
        return Err("That isn't a folder.".into());
    }
    let info = workspace::inspect(&p).await;
    let row = s.core.db.upsert_workspace(&p.to_string_lossy(), &workspace::display_name(&p), info.is_git).map_err(e)?;
    Ok(OpenedWorkspace { row, info })
}

#[tauri::command]
pub async fn inspect_workspace(s: S<'_>, id: String) -> Res<OpenedWorkspace> {
    let row = s.core.db.workspace(&id).map_err(e)?.ok_or("workspace not found")?;
    let info = workspace::inspect(Path::new(&row.path)).await;
    Ok(OpenedWorkspace { row, info })
}

#[tauri::command]
pub fn recent_workspaces(s: S) -> Res<Vec<WorkspaceRow>> {
    s.core.db.recent_workspaces(10).map_err(e)
}

#[tauri::command]
pub fn update_workspace(s: S, row: WorkspaceRow) -> Res<WorkspaceRow> {
    s.core.db.update_workspace(&row).map_err(e)?;
    s.core.db.workspace(&row.id).map_err(e)?.ok_or_else(|| "workspace not found".into())
}

#[tauri::command]
pub fn remove_workspace(s: S, id: String) -> Res<()> {
    s.core.db.remove_workspace(&id).map_err(e)
}

#[tauri::command]
pub async fn init_git(s: S<'_>, id: String) -> Res<OpenedWorkspace> {
    let row = s.core.db.workspace(&id).map_err(e)?.ok_or("workspace not found")?;
    let p = PathBuf::from(&row.path);
    workspace::git::init(&p).await.map_err(e)?;
    let mut row = row;
    row.is_git = true;
    s.core.db.update_workspace(&row).map_err(e)?;
    let info = workspace::inspect(&p).await;
    Ok(OpenedWorkspace { row, info })
}

/// Copies the bundled sample project into the data dir and opens it.
#[tauri::command]
pub async fn sample_project(s: S<'_>) -> Res<OpenedWorkspace> {
    let src = s.core.catalog.dir.join("sample-project");
    let dst = s.core.data_dir.join("sample-project");
    if !dst.exists() {
        library::copy_dir(&src, &dst).map_err(e)?;
        let _ = workspace::git::init(&dst).await;
        let _ = workspace::git::commit_all(&dst, "Sample project").await;
    }
    open_workspace(s, dst.to_string_lossy().into_owned()).await
}

// ---------------------------------------------------------------- runs

#[tauri::command]
pub async fn preflight(
    s: S<'_>,
    workspace_id: Option<String>,
    pool_override: Option<Vec<ExecutorEntry>>,
) -> Res<engine::env::PreflightView> {
    let ws = match &workspace_id {
        Some(id) => s.core.db.workspace(id).map_err(e)?,
        None => None,
    };
    Ok(s.core.preflight(ws.as_ref(), pool_override.as_deref()).await)
}

#[tauri::command]
pub fn start_run(s: S, opts: StartOpts) -> Res<String> {
    s.core.start_run(opts).map_err(e)
}

#[tauri::command]
pub fn cancel_run(s: S, run_id: String) -> Res<()> {
    s.core.cancel_run(&run_id).map_err(e)
}

#[tauri::command]
pub fn pause_run(s: S, run_id: String, paused: bool) -> Res<()> {
    s.core.pause_run(&run_id, paused).map_err(e)
}

#[tauri::command]
pub async fn set_run_mode(s: S<'_>, run_id: String, mode: String) -> Res<()> {
    s.core.set_run_mode(&run_id, &mode).await.map_err(e)
}

#[tauri::command]
pub fn reassign_step(s: S, run_id: String, step_id: String, assignment: Assignment) -> Res<StepRow> {
    s.core.reassign(&run_id, &step_id, assignment).map_err(e)
}

#[tauri::command]
pub fn answer_approval(s: S, id: String, option: String) -> Res<()> {
    s.core.answer(&id, &option).map_err(e)
}

#[tauri::command]
pub fn pending_approvals(s: S) -> Vec<ApprovalRequest> {
    s.core.pending_approvals()
}

#[derive(Serialize)]
pub struct RunDetail {
    pub run: RunRow,
    pub steps: Vec<StepRow>,
    pub active: bool,
    pub paused: bool,
    pub workspace: Option<WorkspaceRow>,
}

#[tauri::command]
pub fn get_run(s: S, run_id: String) -> Res<RunDetail> {
    let run = s.core.db.run(&run_id).map_err(e)?.ok_or("run not found")?;
    let ctl = s.core.run_ctl(&run_id);
    Ok(RunDetail {
        workspace: s.core.db.workspace(&run.workspace_id).map_err(e)?,
        steps: s.core.db.steps(&run_id).map_err(e)?,
        active: ctl.is_some(),
        paused: ctl.is_some_and(|c| c.paused.load(std::sync::atomic::Ordering::SeqCst)),
        run,
    })
}

#[tauri::command]
pub fn list_runs(s: S, workspace_id: Option<String>, since: Option<i64>, limit: Option<usize>) -> Res<Vec<RunRow>> {
    s.core.db.runs(workspace_id.as_deref(), since, limit.unwrap_or(200)).map_err(e)
}

#[tauri::command]
pub fn step_logs(s: S, run_id: String, step_id: String, step_idx: i64, after_seq: u64) -> Vec<LogLine> {
    s.core.logs.get(&run_id, &step_id, step_idx, after_seq)
}

#[tauri::command]
pub async fn rollback_step(s: S<'_>, run_id: String, idx: i64) -> Res<usize> {
    engine::run::rollback(&s.core, &run_id, idx).await.map_err(e)
}

#[tauri::command]
pub async fn accept_run(s: S<'_>, run_id: String, merge: bool, restore_stash: bool) -> Res<String> {
    engine::run::accept(&s.core, &run_id, merge, restore_stash).await.map_err(e)
}

/// Exports a run (with its Library snapshot hash and models) as JSON.
#[tauri::command]
pub fn export_run(s: S, run_id: String, path: String) -> Res<()> {
    let run = s.core.db.run(&run_id).map_err(e)?.ok_or("run not found")?;
    let steps = s.core.db.steps(&run_id).map_err(e)?;
    let models: Vec<String> = steps
        .iter()
        .filter_map(|x| x.model_id.clone().or(x.agent_id.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let events: Vec<_> = steps.iter().flat_map(|st| s.core.db.events_for_step(&st.id, 5000).unwrap_or_default()).collect();
    let doc =
        serde_json::json!({ "exported_by": crate::brand::PRODUCT_NAME, "run": run, "steps": steps, "models": models, "events": events });
    std::fs::write(path, serde_json::to_vec_pretty(&doc).map_err(e)?).map_err(e)
}

#[tauri::command]
pub async fn stop_everything(s: S<'_>) -> Res<String> {
    Ok(s.core.stop_everything().await)
}

// ---------------------------------------------------------------- library

#[derive(Serialize)]
pub struct LibraryView {
    pub items: Vec<library::Item>,
    pub profiles: Vec<serde_json::Value>,
    pub targets: Vec<String>,
    pub conflicts: Vec<sync::Conflict>,
    pub support: serde_json::Value,
    pub import_candidates: Vec<String>,
    pub global_root: String,
}

fn ws_path(s: &S, workspace_id: &Option<String>) -> Res<Option<PathBuf>> {
    match workspace_id {
        Some(id) => Ok(s.core.db.workspace(id).map_err(e)?.map(|w| PathBuf::from(w.path))),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn library_list(s: S, workspace_id: Option<String>) -> Res<LibraryView> {
    let ws = ws_path(&s, &workspace_id)?;
    let items = library::list(&s.core.data_dir, ws.as_deref());
    let targets = s.core.sync_targets();
    let mut support = serde_json::Map::new();
    for cli in sync::CLIS {
        support.insert(
            cli.to_string(),
            serde_json::json!({ "rule": sync::support(cli, Kind::Rule), "skill": sync::support(cli, Kind::Skill), "agent": sync::support(cli, Kind::Agent) }),
        );
    }
    // Conflicts: generated files whose hash no longer matches.
    let mut conflicts = vec![];
    if let Some(w) = &ws {
        let m = sync::load_managed(w);
        for (rel, h) in &m.files {
            if let Ok(b) = std::fs::read(w.join(rel)) {
                if &sync::hash(&b) != h {
                    conflicts.push(sync::Conflict { path: rel.clone(), reason: "edited".into() });
                }
            }
        }
    }
    let profiles = s
        .core
        .db
        .profiles()
        .map_err(e)?
        .into_iter()
        .map(|(id, name, items)| serde_json::json!({"id": id, "display_name": name, "item_ids": items}))
        .collect();
    Ok(LibraryView {
        import_candidates: ws.as_deref().map(library::import::candidates).unwrap_or_default(),
        global_root: library::global_root(&s.core.data_dir).to_string_lossy().into_owned(),
        items,
        profiles,
        targets,
        conflicts,
        support: serde_json::Value::Object(support),
    })
}

fn find_item(s: &S, path: &str, workspace_id: &Option<String>) -> Res<library::Item> {
    let ws = ws_path(s, workspace_id)?;
    library::list(&s.core.data_dir, ws.as_deref()).into_iter().find(|i| i.path == path).ok_or_else(|| "item not found".into())
}

fn library_changed(s: &S, workspace_id: &Option<String>) {
    s.core.bus.send(UiEvent::Library { workspace: workspace_id.clone() });
}

#[tauri::command]
pub fn library_create(s: S, kind: Kind, scope: Scope, display_name: String, workspace_id: Option<String>) -> Res<library::Item> {
    let ws = ws_path(&s, &workspace_id)?;
    let root = library::root_for(scope, &s.core.data_dir, ws.as_deref()).map_err(e)?;
    let item = library::create(&root, kind, scope, &display_name).map_err(e)?;
    library_changed(&s, &workspace_id);
    Ok(item)
}

/// Saves raw editor text; when the Library screen is open the workspace is re-synced.
#[tauri::command]
pub fn library_save(s: S, path: String, raw: String, workspace_id: Option<String>) -> Res<Option<sync::SyncReport>> {
    let item = find_item(&s, &path, &workspace_id)?;
    library::save_raw(Path::new(&item.path), &raw).map_err(e)?;
    let report = match &workspace_id {
        Some(_) => Some(library_sync_inner(&s, &workspace_id)?),
        None => None,
    };
    library_changed(&s, &workspace_id);
    Ok(report)
}

#[tauri::command]
pub fn library_rename(s: S, path: String, display_name: String, workspace_id: Option<String>) -> Res<()> {
    let item = find_item(&s, &path, &workspace_id)?;
    library::rename(&item, &display_name).map_err(e)?;
    library_changed(&s, &workspace_id);
    Ok(())
}

#[tauri::command]
pub fn library_toggle(s: S, path: String, enabled: bool, workspace_id: Option<String>) -> Res<()> {
    let item = find_item(&s, &path, &workspace_id)?;
    library::set_enabled(&item, enabled).map_err(e)?;
    library_changed(&s, &workspace_id);
    Ok(())
}

#[tauri::command]
pub fn library_references(s: S, path: String, workspace_id: Option<String>) -> Res<Vec<String>> {
    let ws = ws_path(&s, &workspace_id)?;
    let items = library::list(&s.core.data_dir, ws.as_deref());
    let item = items.iter().find(|i| i.path == path).ok_or("item not found")?;
    Ok(library::referenced_by(&items, item))
}

#[tauri::command]
pub fn library_delete(s: S, path: String, workspace_id: Option<String>) -> Res<()> {
    let item = find_item(&s, &path, &workspace_id)?;
    library::delete(&item).map_err(e)?;
    library_changed(&s, &workspace_id);
    Ok(())
}

#[tauri::command]
pub fn library_duplicate(s: S, path: String, workspace_id: Option<String>) -> Res<String> {
    let item = find_item(&s, &path, &workspace_id)?;
    let root = Path::new(&item.path).ancestors().nth(if item.kind == Kind::Skill { 3 } else { 2 }).ok_or("bad path")?.to_path_buf();
    let p = library::copy_to(&item, &root, Some(&format!("{} copy", item.display_name))).map_err(e)?;
    library_changed(&s, &workspace_id);
    Ok(p.to_string_lossy().into_owned())
}

/// Moves (or copies) an item between Global and Workspace scope.
#[tauri::command]
pub fn library_move(s: S, path: String, to: Scope, keep_original: bool, workspace_id: Option<String>) -> Res<String> {
    let item = find_item(&s, &path, &workspace_id)?;
    let ws = ws_path(&s, &workspace_id)?;
    let root = library::root_for(to, &s.core.data_dir, ws.as_deref()).map_err(e)?;
    let p = library::copy_to(&item, &root, None).map_err(e)?;
    if !keep_original {
        library::delete(&item).map_err(e)?;
    }
    library_changed(&s, &workspace_id);
    Ok(p.to_string_lossy().into_owned())
}

#[derive(Serialize)]
pub struct Preview {
    pub cli: String,
    pub support: sync::Support,
    pub text: String,
    pub installed: bool,
}

#[tauri::command]
pub fn library_preview(s: S, path: String, workspace_id: Option<String>) -> Res<Vec<Preview>> {
    let ws = ws_path(&s, &workspace_id)?;
    let items = library::list(&s.core.data_dir, ws.as_deref());
    let item = items.iter().find(|i| i.path == path).ok_or("item not found")?;
    let targets = s.core.sync_targets();
    Ok(sync::CLIS
        .iter()
        .map(|cli| {
            let (support, text) = sync::preview(item, &items, cli);
            Preview { cli: cli.to_string(), support, text, installed: targets.iter().any(|t| t == cli) }
        })
        .collect())
}

fn library_sync_inner(s: &S, workspace_id: &Option<String>) -> Res<sync::SyncReport> {
    let id = workspace_id.as_ref().ok_or("open a workspace first")?;
    let row = s.core.db.workspace(id).map_err(e)?.ok_or("workspace not found")?;
    let ws = PathBuf::from(&row.path);
    let items = library::list(&s.core.data_dir, Some(&ws));
    let profile = row.library_profile_id.as_ref().and_then(|pid| s.core.db.profiles().ok()?.into_iter().find(|p| &p.0 == pid).map(|p| p.2));
    let resolved = library::resolve(&items, profile.as_deref());
    let targets = s.core.sync_targets();
    let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
    sync::apply(&ws, &sync::plan(&resolved, &refs), &refs, row.settings.commit_generated).map_err(e)
}

#[tauri::command]
pub fn library_sync(s: S, workspace_id: Option<String>) -> Res<sync::SyncReport> {
    library_sync_inner(&s, &workspace_id)
}

#[tauri::command]
pub fn library_import(s: S, workspace_id: String, scope: Scope) -> Res<usize> {
    let wid = Some(workspace_id);
    let ws = ws_path(&s, &wid)?.ok_or("workspace not found")?;
    let root = library::root_for(scope, &s.core.data_dir, Some(&ws)).map_err(e)?;
    let n = library::import::import_all(&ws, &root).map_err(e)?;
    if let Some(mut row) = s.core.db.workspace(wid.as_ref().unwrap()).map_err(e)? {
        row.settings.import_offered = true;
        let _ = s.core.db.update_workspace(&row);
    }
    library_changed(&s, &wid);
    Ok(n)
}

#[tauri::command]
pub fn library_import_edit(s: S, workspace_id: String, rel: String) -> Res<Option<String>> {
    let wid = Some(workspace_id);
    let ws = ws_path(&s, &wid)?.ok_or("workspace not found")?;
    let items = library::list(&s.core.data_dir, Some(&ws));
    let r = library::import::import_edit(&ws, &rel, &items).map_err(e)?;
    library_changed(&s, &wid);
    Ok(r)
}

#[tauri::command]
pub fn save_profile(s: S, id: Option<String>, display_name: String, item_ids: Vec<String>) -> Res<String> {
    let id = id.unwrap_or_else(|| library::slugify(&display_name));
    s.core.db.save_profile(&id, &display_name, &item_ids).map_err(e)?;
    Ok(id)
}

#[tauri::command]
pub fn delete_profile(s: S, id: String) -> Res<()> {
    s.core.db.delete_profile(&id).map_err(e)
}

// ---------------------------------------------------------------- agents

#[tauri::command]
pub async fn agents(s: S<'_>, refresh: bool, auth: bool) -> Res<Vec<engine::AgentInfo>> {
    if refresh {
        Ok(s.core.refresh_agents(auth).await)
    } else {
        Ok(s.core.agent_infos())
    }
}

#[tauri::command]
pub fn agent_set(s: S, id: String, display_name: Option<String>, enabled: Option<bool>) -> Res<()> {
    s.core.db.set_agent_meta(&id, display_name.as_deref(), enabled).map_err(e)?;
    s.core.bus.send(UiEvent::Agents);
    Ok(())
}

fn spawn_job(core: Arc<Core>, job_id: String, command: String, after: impl FnOnce(Arc<Core>, bool) + Send + 'static) {
    let cancel = CancelToken::default();
    core.jobs.lock().insert(job_id.clone(), cancel.clone());
    tauri::async_runtime::spawn(async move {
        let c2 = core.clone();
        let jid = job_id.clone();
        let r = installer::run_install(&command, &core.registry, &format!("install:{job_id}"), &cancel, move |l| {
            c2.bus.send(UiEvent::Install { id: jid.clone(), line: l.to_string(), done: None, progress: None });
        })
        .await;
        core.jobs.lock().remove(&job_id);
        let ok = r.is_ok();
        let line = match r {
            Ok(()) => "Done.".to_string(),
            Err(err) => format!("Failed: {err:#}"),
        };
        core.bus.send(UiEvent::Install { id: job_id, line, done: Some(ok), progress: None });
        after(core, ok);
    });
}

/// Runs the catalog install command the user saw and confirmed.
#[tauri::command]
pub fn install_agent(s: S, id: String, option: usize) -> Res<String> {
    let cat = s.core.catalog.agent(&id).ok_or("unknown agent")?;
    let opt = cat.install.get(os_key()).and_then(|o| o.get(option)).ok_or("no install option for this OS")?;
    spawn_job(s.core.clone(), id.clone(), opt.command.clone(), |core, _| {
        tauri::async_runtime::spawn(async move {
            core.refresh_agents(true).await;
            core.bus.send(UiEvent::Agents);
        });
    });
    Ok(opt.command.clone())
}

#[tauri::command]
pub fn uninstall_agent(s: S, id: String) -> Res<String> {
    let cat = s.core.catalog.agent(&id).ok_or("unknown agent")?;
    let cmd = cat.uninstall.get(os_key()).cloned().ok_or("no uninstall command for this OS")?;
    spawn_job(s.core.clone(), id, cmd.clone(), |core, _| {
        tauri::async_runtime::spawn(async move {
            core.refresh_agents(false).await;
            core.bus.send(UiEvent::Agents);
        });
    });
    Ok(cmd)
}

#[tauri::command]
pub fn install_runtime(s: S, id: String, option: usize) -> Res<String> {
    let rt = s.core.catalog.runtimes.iter().find(|r| r.id == id).ok_or("unknown runtime")?;
    let opt = rt.install.get(os_key()).and_then(|o| o.get(option)).ok_or("no install option for this OS")?;
    spawn_job(s.core.clone(), id, opt.command.clone(), |core, _| core.bus.send(UiEvent::Agents));
    Ok(opt.command.clone())
}

#[tauri::command]
pub fn cancel_job(s: S, id: String) -> Res<()> {
    if let Some(c) = s.core.jobs.lock().get(&id) {
        c.cancel();
    }
    Ok(())
}

/// Opens the CLI's own login flow in Terminal (the app never stores paid-agent credentials).
#[tauri::command]
pub fn open_login(s: S, id: String) -> Res<()> {
    let a = s.core.adapter(&id).ok_or("unknown agent")?;
    let cmd = a.login_command().ok_or("this agent has no login step")?;
    open_in_terminal(&cmd)
}

fn open_in_terminal(cmd: &str) -> Res<()> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\"\n activate\n do script \"{}\"\nend tell",
            cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );
        std::process::Command::new("osascript").args(["-e", &script]).spawn().map_err(e)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(format!("Run this in a terminal: {cmd}"))
    }
}

// ---------------------------------------------------------------- local models

#[derive(Serialize, Clone)]
pub struct LocalModel {
    pub name: String,
    pub size_gb: f64,
    pub loaded: bool,
    pub mem_gb: f64,
    pub ctx: u32,
    pub fit: governor::Fit,
    pub catalog: Option<installer::catalog::CatalogModel>,
    pub in_pool: bool,
}

async fn local_models_inner(c: &Arc<Core>) -> Vec<LocalModel> {
    let settings = c.db.settings();
    let ls = c.local_state(&settings).await;
    let total = crate::macos::hardware().total_mem_gb;
    let budget = governor::budget_gb(total, settings.memory_budget_pct);
    let mut v: Vec<LocalModel> = ls
        .ollama_models
        .iter()
        .map(|(name, size)| {
            let cat = c.catalog.model(name).or_else(|| c.catalog.model(name.trim_end_matches(":latest"))).cloned();
            let ctx = governor::choose_ctx(*size, total, budget);
            let mem = cat.as_ref().filter(|_| ctx >= 16_384).map(|m| m.mem_gb).unwrap_or_else(|| governor::estimate_mem_gb(*size, ctx));
            LocalModel {
                loaded: ls.ollama_loaded.iter().any(|l| l == name),
                fit: governor::fit(mem, total * 0.75, budget, false),
                in_pool: settings.executor_pool.iter().any(|e| e.model.tier == Tier::Local && &e.model.name == name),
                name: name.clone(),
                size_gb: (*size * 100.0).round() / 100.0,
                mem_gb: mem,
                ctx,
                catalog: cat,
            }
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

#[tauri::command]
pub async fn local_models(s: S<'_>) -> Res<Vec<LocalModel>> {
    Ok(local_models_inner(&s.core).await)
}

#[tauri::command]
pub fn hardware_check(s: S) -> HardwareCheck {
    installer::hardware_check(&s.core.db.settings())
}

#[tauri::command]
pub async fn model_suggestions(s: S<'_>) -> Res<Vec<Suggestion>> {
    let hw = installer::hardware_check(&s.core.db.settings());
    let installed: Vec<String> = local_models_inner(&s.core).await.into_iter().map(|m| m.name).collect();
    Ok(installer::suggestions(&s.core.catalog, &hw, &installed))
}

#[tauri::command]
pub async fn ollama_status(s: S<'_>) -> Res<OllamaStatus> {
    Ok(s.core.ollama.status(&s.core.db.settings()).await)
}

#[tauri::command]
pub async fn ollama_start(s: S<'_>) -> Res<OllamaStatus> {
    let settings = s.core.db.settings();
    let mut st = settings.clone();
    st.manage_ollama = true;
    s.core.ollama.ensure_running(&st, &s.core.registry, crate::macos::hardware().total_mem_gb).await.map_err(e)?;
    Ok(s.core.ollama.status(&settings).await)
}

#[tauri::command]
pub async fn ollama_stop(s: S<'_>) -> Res<OllamaStatus> {
    s.core.ollama.stop(&s.core.registry).await;
    Ok(s.core.ollama.status(&s.core.db.settings()).await)
}

/// Downloads a model through the runtime's pull API with streamed progress.
/// Pausing = cancelling; Ollama keeps partial downloads so a later pull resumes.
#[tauri::command]
pub async fn pull_model(s: S<'_>, name: String) -> Res<()> {
    // Returns at once; starting Ollama and the download report progress as events.
    let core = s.core.clone();
    let cancel = CancelToken::default();
    if core.pulls.lock().insert(name.clone(), cancel.clone()).is_some() {
        return Err("Already downloading.".into());
    }
    log::info!("pull {name}: starting");
    tauri::async_runtime::spawn(async move {
        let settings = core.db.settings();
        if core.ollama.client().version().await.is_none() {
            core.bus.send(UiEvent::Pull { model: name.clone(), status: "Starting Ollama…".into(), completed: 0, total: 0, done: None, error: None });
        }
        if let Err(err) = core.ollama.ensure_running(&settings, &core.registry, crate::macos::hardware().total_mem_gb).await {
            core.pulls.lock().remove(&name);
            log::warn!("pull {name}: {err:#}");
            core.bus.send(UiEvent::Pull { model: name, status: "stopped".into(), completed: 0, total: 0, done: Some(false), error: Some(format!("{err:#}")) });
            return;
        }
        core.bus.send(UiEvent::Pull { model: name.clone(), status: "Connecting to the model registry…".into(), completed: 0, total: 0, done: None, error: None });
        let client = core.ollama.client();
        let c2 = core.clone();
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let r = client
            .pull(&name, &cancel, |p| {
                // Progress at most ~4×/s.
                if last.elapsed().as_millis() > 250 || p.total > 0 && p.completed == p.total {
                    last = std::time::Instant::now();
                    c2.bus.send(UiEvent::Pull {
                        model: p.model,
                        status: p.status,
                        completed: p.completed,
                        total: p.total,
                        done: None,
                        error: None,
                    });
                }
            })
            .await;
        core.pulls.lock().remove(&name);
        log::info!("pull {name}: {}", match &r { Ok(()) => "done".to_string(), Err(e) => format!("{e:#}") });
        let (done, error) = match r {
            Ok(()) => (Some(true), None),
            Err(err) if cancel.is_cancelled() => (Some(false), Some(format!("Paused: {err}"))),
            Err(err) => (Some(false), Some(format!("{err:#}"))),
        };
        core.bus.send(UiEvent::Pull {
            model: name,
            status: if done == Some(true) { "success".into() } else { "stopped".into() },
            completed: 0,
            total: 0,
            done,
            error,
        });
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_pull(s: S, name: String) -> Res<()> {
    if let Some(c) = s.core.pulls.lock().get(&name) {
        c.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_model(s: S<'_>, name: String) -> Res<f64> {
    let settings = s.core.db.settings();
    let size = local_models_inner(&s.core).await.into_iter().find(|m| m.name == name).map(|m| m.size_gb).unwrap_or(0.0);
    s.core.ollama.ensure_running(&settings, &s.core.registry, crate::macos::hardware().total_mem_gb).await.map_err(e)?;
    s.core.ollama.client().delete(&name).await.map_err(e)?;
    let mut st = s.core.db.settings();
    st.executor_pool.retain(|x| !(x.model.tier == Tier::Local && x.model.name == name));
    s.core.db.save_settings(&st).map_err(e)?;
    Ok(size)
}

#[tauri::command]
pub async fn unload_model(s: S<'_>, name: String) -> Res<f64> {
    s.core.unload_model(&name).await.map_err(e)
}

#[tauri::command]
pub async fn set_model_folder(s: S<'_>, path: Option<String>) -> Res<OllamaStatus> {
    let mut st = s.core.db.settings();
    if let Some(p) = &path {
        let pb = PathBuf::from(p);
        std::fs::create_dir_all(&pb).map_err(e)?;
    }
    st.model_folder = path;
    s.core.db.save_settings(&st).map_err(e)?;
    // Restart a server we manage so it picks up the new folder.
    if s.core.ollama.managed() {
        s.core.ollama.stop(&s.core.registry).await;
        s.core.ollama.ensure_running(&st, &s.core.registry, crate::macos::hardware().total_mem_gb).await.map_err(e)?;
    }
    Ok(s.core.ollama.status(&st).await)
}

/// Moves every model to a new folder (copy → verify → remove old), then switches the setting.
#[tauri::command]
pub async fn move_models(s: S<'_>, to: String) -> Res<()> {
    let core = s.core.clone();
    let settings = core.db.settings();
    let from = crate::governor::service::OllamaService::models_dir(&settings);
    let to = PathBuf::from(to);
    let cancel = CancelToken::default();
    core.jobs.lock().insert("move-models".into(), cancel.clone());
    if core.ollama.managed() {
        core.ollama.stop(&core.registry).await;
    }
    tauri::async_runtime::spawn(async move {
        let c2 = core.clone();
        let mut last = std::time::Instant::now();
        let r = installer::move_models(&from, &to, &cancel, |d, t| {
            if last.elapsed().as_millis() > 200 || d == t {
                last = std::time::Instant::now();
                c2.bus.send(UiEvent::Move { completed: d, total: t, done: None, error: None });
            }
        })
        .await;
        core.jobs.lock().remove("move-models");
        match r {
            Ok(()) => {
                let mut st = core.db.settings();
                st.model_folder = Some(to.to_string_lossy().into_owned());
                let _ = core.db.save_settings(&st);
                core.bus.send(UiEvent::Move { completed: 1, total: 1, done: Some(true), error: None });
            }
            Err(err) => core.bus.send(UiEvent::Move { completed: 0, total: 0, done: Some(false), error: Some(format!("{err:#}")) }),
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn import_gguf(s: S<'_>, path: String, name: String) -> Res<()> {
    let core = s.core.clone();
    let settings = core.db.settings();
    core.ollama.ensure_running(&settings, &core.registry, crate::macos::hardware().total_mem_gb).await.map_err(e)?;
    let cancel = CancelToken::default();
    let c2 = core.clone();
    let n2 = name.clone();
    installer::import_gguf(Path::new(&path), &name, &core.registry, &cancel, move |l| {
        c2.bus.send(UiEvent::Install { id: format!("import:{n2}"), line: l.to_string(), done: None, progress: None });
    })
    .await
    .map_err(e)
}

// ---------------------------------------------------------------- providers & pool

#[derive(Serialize)]
pub struct ProviderView {
    #[serde(flatten)]
    pub row: ProviderRow,
    pub key_hint: Option<String>,
}

#[tauri::command]
pub fn providers_list(s: S) -> Res<Vec<ProviderView>> {
    Ok(s.core
        .db
        .providers()
        .map_err(e)?
        .into_iter()
        .map(|row| ProviderView { key_hint: row.key_ref.as_deref().and_then(keychain::hint), row })
        .collect())
}

#[derive(Deserialize)]
pub struct ConnectProvider {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: ProviderType,
    pub base_url: String,
    pub key: Option<String>,
}

/// Tests the key, stores it in the Keychain and returns the provider's models.
#[tauri::command]
pub async fn connect_provider(s: S<'_>, p: ConnectProvider) -> Res<Vec<CloudModel>> {
    let base = p.base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("Enter the provider's base URL.".into());
    }
    let key = p.key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    let id = library::slugify(&p.id);
    let key_ref = format!("provider:{id}");
    let key = match key {
        Some(k) => Some(k),
        None => keychain::get(&key_ref),
    };
    if let Some(k) = &key {
        providers::test_key(p.kind, &base, k).await.map_err(e)?;
        keychain::set(&key_ref, k).map_err(|err| format!("Couldn't save the key to the Keychain: {err}"))?;
    }
    let models = providers::list_models(p.kind, &base, key.as_deref()).await.map_err(e)?;
    s.core
        .db
        .save_provider(&ProviderRow {
            id,
            display_name: p.display_name,
            kind: p.kind.to_string(),
            base_url: Some(base),
            key_ref: key.map(|_| key_ref),
            enabled: true,
            monthly_cap_usd: None,
        })
        .map_err(e)?;
    Ok(models)
}

#[tauri::command]
pub async fn provider_models(s: S<'_>, id: String) -> Res<Vec<CloudModel>> {
    let p = s.core.db.providers().map_err(e)?.into_iter().find(|p| p.id == id).ok_or("provider not found")?;
    let kind = ProviderType::parse(&p.kind).unwrap_or(ProviderType::OpenaiCompatible);
    let key = p.key_ref.as_deref().and_then(keychain::get);
    providers::list_models(kind, p.base_url.as_deref().unwrap_or(""), key.as_deref()).await.map_err(e)
}

#[tauri::command]
pub fn update_provider(s: S, row: ProviderRow) -> Res<()> {
    s.core.db.save_provider(&row).map_err(e)
}

/// Removes the provider, its Keychain entry and its executor-pool entries.
#[tauri::command]
pub fn disconnect_provider(s: S, id: String) -> Res<()> {
    if let Some(p) = s.core.db.providers().map_err(e)?.into_iter().find(|p| p.id == id) {
        if let Some(k) = &p.key_ref {
            keychain::delete(k).map_err(e)?;
        }
    }
    s.core.db.delete_provider(&id).map_err(e)?;
    let mut st = s.core.db.settings();
    st.executor_pool.retain(|x| x.model.provider_id != id);
    if st.budget_planner.as_ref().is_some_and(|b| b.model.provider_id == id) {
        st.budget_planner = None;
        st.budget_planning = false;
    }
    s.core.db.save_settings(&st).map_err(e)
}

#[derive(Deserialize)]
pub struct ConnectModel {
    pub agent_id: String,
    /// Provider id ("ollama" for local Ollama models).
    pub provider_id: String,
    pub name: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub price_in_per_m: Option<f64>,
    #[serde(default)]
    pub price_out_per_m: Option<f64>,
    #[serde(default)]
    pub ctx_len: Option<u32>,
    #[serde(default)]
    pub tool_calling: Option<bool>,
    /// "pool_front" | "pool_back" | "budget_planner"
    pub target: String,
}

/// Builds a ModelRef for a provider model.
fn model_ref(s: &S, provider_id: &str, name: &str) -> Res<ModelRef> {
    let settings = s.core.db.settings();
    if provider_id == "ollama" {
        let total = crate::macos::hardware().total_mem_gb;
        let budget = governor::budget_gb(total, settings.memory_budget_pct);
        let dir = crate::governor::service::OllamaService::models_dir(&settings);
        let size = providers::scan_ollama_store(&dir)
            .into_iter()
            .find(|(n, _)| n == name || n == &format!("{name}:latest"))
            .map(|(_, s)| s as f64 / 1_073_741_824.0);
        let cat = s.core.catalog.model(name).cloned();
        let size_gb = size.or(cat.as_ref().map(|c| c.download_gb)).unwrap_or(4.5);
        let ctx = governor::choose_ctx(size_gb, total, budget);
        return Ok(ModelRef {
            provider_id: "ollama".into(),
            provider_type: ProviderType::Ollama,
            name: name.into(),
            display_name: cat.as_ref().map(|c| c.display_name.clone()),
            base_url: Some(providers::OLLAMA_DEFAULT.into()),
            key_ref: None,
            tier: Tier::Local,
            mem_needed_gb: Some(governor::estimate_mem_gb(size_gb, ctx)),
            ctx_len: Some(ctx),
            price_in_per_m: Some(0.0),
            price_out_per_m: Some(0.0),
        });
    }
    let p = s.core.db.providers().map_err(e)?.into_iter().find(|p| p.id == provider_id).ok_or("provider not found")?;
    let kind = ProviderType::parse(&p.kind).unwrap_or(ProviderType::OpenaiCompatible);
    Ok(ModelRef {
        provider_id: p.id.clone(),
        provider_type: kind,
        name: name.into(),
        display_name: None,
        base_url: p.base_url.clone(),
        key_ref: p.key_ref.clone(),
        tier: if kind.is_local() { Tier::Local } else { Tier::CheapCloud },
        mem_needed_gb: None,
        ctx_len: None,
        price_in_per_m: None,
        price_out_per_m: None,
    })
}

/// Connects a model to an open-source agent: adds it to the executor pool
/// (or makes it the budget planner) and records it in the models table.
#[tauri::command]
pub async fn connect_model(s: S<'_>, m: ConnectModel) -> Res<Settings> {
    let mut mr = model_ref(&s, &m.provider_id, &m.name)?;
    if m.display_name.is_some() {
        mr.display_name = m.display_name.clone();
    }
    if m.price_in_per_m.is_some() {
        mr.price_in_per_m = m.price_in_per_m;
        mr.price_out_per_m = m.price_out_per_m;
    }
    if m.ctx_len.is_some() && mr.ctx_len.is_none() {
        mr.ctx_len = m.ctx_len;
    }
    let adapter = s.core.adapter(&m.agent_id).ok_or("unknown agent")?;
    adapter.configure_model(&AdapterWs { path: PathBuf::new() }, &mr).await.map_err(e)?;
    s.core
        .db
        .save_model(&ModelRow {
            id: format!("{}/{}", mr.provider_id, mr.name),
            display_name: mr.label(),
            provider_id: mr.provider_id.clone(),
            runtime_id: (mr.tier == Tier::Local).then(|| "ollama".into()),
            name: mr.name.clone(),
            tier: mr.tier.to_string(),
            mem_needed_gb: mr.mem_needed_gb,
            ctx_len: mr.ctx_len.map(|c| c as i64),
            tool_calling: m.tool_calling,
            price_in_per_m: mr.price_in_per_m,
            price_out_per_m: mr.price_out_per_m,
            installed: true,
            ..Default::default()
        })
        .map_err(e)?;
    let entry = ExecutorEntry { agent_id: m.agent_id, model: mr, enabled: true };
    let mut st = s.core.db.settings();
    match m.target.as_str() {
        "budget_planner" => {
            st.budget_planner = Some(entry);
            st.budget_planning = true;
        }
        t => {
            st.executor_pool.retain(|x| {
                !(x.agent_id == entry.agent_id && x.model.provider_id == entry.model.provider_id && x.model.name == entry.model.name)
            });
            if t == "pool_front" {
                st.executor_pool.insert(0, entry);
            } else {
                st.executor_pool.push(entry);
            }
        }
    }
    s.core.db.save_settings(&st).map_err(e)?;
    Ok(st)
}

#[derive(Serialize)]
pub struct SmokeResult {
    pub ok: bool,
    pub message: String,
}

/// 10-second smoke test: the agent is installed and the model answers.
#[tauri::command]
pub async fn smoke_test(s: S<'_>, entry: ExecutorEntry) -> Res<SmokeResult> {
    let agent = s.core.adapter(&entry.agent_id).ok_or("unknown agent")?;
    let d = agent.detect().await;
    if !d.installed {
        return Ok(SmokeResult { ok: false, message: format!("{} isn't installed yet.", agent.display_name()) });
    }
    let m = &entry.model;
    if m.provider_type == ProviderType::Ollama {
        let settings = s.core.db.settings();
        s.core.ollama.ensure_running(&settings, &s.core.registry, crate::macos::hardware().total_mem_gb).await.map_err(e)?;
    }
    if m.provider_type == ProviderType::Ollama {
        log::info!("smoke test: {} + {}", agent.display_name(), m.name);
        return Ok(match s.core.ollama.client().quick_check(&m.name).await {
            Ok(secs) => {
                s.core.governor.mark_loaded(&m.name);
                SmokeResult { ok: true, message: format!("{} + {}: answered in {secs:.1}s", agent.display_name(), m.label()) }
            }
            Err(err) => SmokeResult { ok: false, message: format!("{err:#}") },
        });
    }
    let base = m.base_url.clone().unwrap_or_default();
    let key = m.key_ref.as_deref().and_then(keychain::get);
    match providers::smoke_test(m.provider_type, &base, key.as_deref(), &m.name).await {
        Ok(msg) => {
            if m.provider_type == ProviderType::Ollama {
                s.core.governor.mark_loaded(&m.name);
            }
            Ok(SmokeResult { ok: true, message: format!("{} + {}: {msg}", agent.display_name(), m.label()) })
        }
        Err(err) => Ok(SmokeResult { ok: false, message: format!("{err:#}") }),
    }
}

#[tauri::command]
pub fn models_list(s: S) -> Res<Vec<ModelRow>> {
    s.core.db.models().map_err(e)
}

/// Renames a model or edits its prices everywhere it's used.
#[tauri::command]
pub fn update_model(s: S, row: ModelRow) -> Res<Settings> {
    s.core.db.save_model(&row).map_err(e)?;
    let mut st = s.core.db.settings();
    let fix = |m: &mut ModelRef| {
        if m.provider_id == row.provider_id && m.name == row.name {
            m.display_name = Some(row.display_name.clone());
            m.price_in_per_m = row.price_in_per_m;
            m.price_out_per_m = row.price_out_per_m;
        }
    };
    st.executor_pool.iter_mut().for_each(|x| fix(&mut x.model));
    if let Some(b) = st.budget_planner.as_mut() {
        fix(&mut b.model);
    }
    s.core.db.save_settings(&st).map_err(e)?;
    Ok(st)
}

// ---------------------------------------------------------------- telemetry

/// Opens/closes the Resource view; sampling runs only while it's open or a run is active.
#[tauri::command]
pub async fn resources_view(s: S<'_>, open: bool) -> Res<Snapshot> {
    let was = s.core.resource_view_open.swap(open, std::sync::atomic::Ordering::SeqCst);
    match (was, open) {
        (false, true) => s.core.sampler.acquire(),
        (true, false) => s.core.sampler.release(),
        _ => {}
    }
    Ok(s.core.sampler.sample(false).await)
}

#[tauri::command]
pub async fn resource_snapshot(s: S<'_>) -> Res<Snapshot> {
    Ok(s.core.sampler.sample(false).await)
}

#[derive(Serialize)]
pub struct HeavyProcess {
    pub name: String,
    pub pid: u32,
    pub rss_mb: f64,
    pub cpu_pct: f64,
    pub controlled: bool,
}

#[derive(Serialize)]
pub struct WhySlow {
    pub snapshot: Snapshot,
    pub others: Vec<HeavyProcess>,
    pub advice: Vec<String>,
}

/// "Why is my Mac slow?": what the app controls (with Unload/Stop) plus
/// heavy processes it doesn't control. One user-triggered full scan.
#[tauri::command]
pub async fn why_slow(s: S<'_>) -> Res<WhySlow> {
    let snap = s.core.sampler.sample(false).await;
    let controlled: std::collections::HashSet<u32> =
        s.core.registry.list().into_iter().flat_map(|p| crate::macos::process_tree(p.pid)).chain([std::process::id()]).collect();
    let others = tokio::task::spawn_blocking(move || {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, sysinfo::ProcessRefreshKind::nothing().with_memory());
        let mut v: Vec<HeavyProcess> = sys
            .processes()
            .iter()
            .map(|(pid, p)| HeavyProcess {
                name: p.name().to_string_lossy().into_owned(),
                pid: pid.as_u32(),
                rss_mb: p.memory() as f64 / 1_048_576.0,
                cpu_pct: 0.0,
                controlled: controlled.contains(&pid.as_u32()),
            })
            .filter(|p| !p.controlled)
            .collect();
        v.sort_by(|a, b| b.rss_mb.partial_cmp(&a.rss_mb).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(6);
        v
    })
    .await
    .map_err(e)?;
    let mut advice = vec![];
    if snap.models.iter().any(|m| m.mem_mb > 500.0) {
        advice.push("Unload idle local models to free memory right away.".into());
    }
    if snap.sys.free_gb < 1.5 {
        advice.push("Free memory is low. Prefer a cloud model for cheap steps until it recovers.".into());
    }
    if matches!(snap.sys.thermal, Some(crate::macos::Thermal::Serious | crate::macos::Thermal::Critical)) {
        advice.push("Your Mac is hot. Cheap steps will prefer cloud until it cools.".into());
    }
    if advice.is_empty() {
        advice.push("Nothing the app controls is using much right now.".into());
    }
    Ok(WhySlow { snapshot: snap, others, advice })
}

#[tauri::command]
pub fn metrics_since(s: S, since: i64) -> Res<Vec<crate::db::queries::MetricRow>> {
    s.core.db.metrics_since(since).map_err(e)
}

#[tauri::command]
pub fn outcome_stats(s: S, workspace_id: Option<String>) -> Res<Vec<crate::db::queries::OutcomeStat>> {
    s.core.db.outcome_stats(workspace_id.as_deref()).map_err(e)
}

/// Installs the `orch <path>` shell command into ~/.local/bin.
#[tauri::command]
pub fn install_cli_command() -> Res<String> {
    let exe = std::env::current_exe().map_err(e)?;
    let dir = dirs::home_dir().ok_or("no home dir")?.join(".local/bin");
    std::fs::create_dir_all(&dir).map_err(e)?;
    let p = dir.join(crate::brand::CLI_NAME);
    let script = format!(
        "#!/bin/sh\n# Opens a folder in {}.\nDIR=\"$(cd \"${{1:-.}}\" && pwd)\"\n\"{}\" \"$DIR\" >/dev/null 2>&1 &\n",
        crate::brand::PRODUCT_NAME,
        exe.display()
    );
    std::fs::write(&p, script).map_err(e)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).map_err(e)?;
    }
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn set_window_visible(s: S, visible: bool) {
    s.core.window_visible.store(visible, std::sync::atomic::Ordering::SeqCst);
}

// ---------------------------------------------------------------- workflows (M5)

fn workflows_dir(s: &S, workspace_id: &Option<String>) -> Res<PathBuf> {
    match ws_path(s, workspace_id)? {
        Some(w) => Ok(crate::handoff::dir(&w).join("workflows")),
        None => Ok(s.core.data_dir.join("workflows")),
    }
}

/// Workflows are JSON files in `.orchestrator/workflows/` (versioned with the workspace).
#[tauri::command]
pub fn workflow_list(s: S, workspace_id: Option<String>) -> Res<Vec<serde_json::Value>> {
    let dir = workflows_dir(&s, &workspace_id)?;
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().is_some_and(|x| x == "json") {
                if let Some(v) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
                    out.push(v);
                }
            }
        }
    }
    out.sort_by_key(|v| v.get("display_name").and_then(|n| n.as_str()).unwrap_or("").to_lowercase());
    Ok(out)
}

#[tauri::command]
pub fn workflow_save(s: S, workspace_id: Option<String>, workflow: serde_json::Value) -> Res<()> {
    let id = workflow.get("id").and_then(|v| v.as_str()).map(library::slugify).ok_or("workflow needs an id")?;
    let dir = workflows_dir(&s, &workspace_id)?;
    std::fs::create_dir_all(&dir).map_err(e)?;
    std::fs::write(dir.join(format!("{id}.json")), serde_json::to_vec_pretty(&workflow).map_err(e)?).map_err(e)
}

#[tauri::command]
pub fn workflow_delete(s: S, workspace_id: Option<String>, id: String) -> Res<()> {
    let dir = workflows_dir(&s, &workspace_id)?;
    let _ = std::fs::remove_file(dir.join(format!("{}.json", library::slugify(&id))));
    Ok(())
}
