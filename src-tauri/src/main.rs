// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Test/demo helpers that never open a window.
    match args.first().map(String::as_str) {
        Some("--fake-agent") => std::process::exit(orchestrator_lib::adapters::fake_cli::main(&args[1..])),
        Some("--fake-provider") => {
            let port = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
            orchestrator_lib::run_fake_provider(port);
            return;
        }
        _ => {}
    }
    orchestrator_lib::run()
}
