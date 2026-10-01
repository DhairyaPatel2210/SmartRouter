pub mod adapters;
pub mod brand;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
