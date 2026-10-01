//! Opt-in: a full run where the cheap step executes on the real OpenCode CLI
//! with a real local Ollama model (planning uses the scripted fake agent, so
//! nothing is spent). Needs opencode + ollama + the model installed:
//!   ORCH_REAL_MODEL=qwen2.5-coder:3b cargo test --test real_local_run -- --ignored --nocapture

use orchestrator_lib::engine::{Core, CoreDeps, StartOpts};
use orchestrator_lib::telemetry::{Emitter, UiEvent};
use orchestrator_lib::types::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Quiet;
impl Emitter for Quiet {
    fn emit(&self, _: Vec<UiEvent>) {}
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn local_step_runs_on_opencode_with_ollama() {
    std::env::set_var("ORCH_FAKE_AGENT_BIN", env!("CARGO_BIN_EXE_orchestrator"));
    let model = std::env::var("ORCH_REAL_MODEL").unwrap_or_else(|_| "qwen2.5-coder:3b".into());
    let data = tempfile::tempdir().unwrap();
    let wsd = tempfile::tempdir().unwrap();
    let ws = wsd.path().join("project");
    std::fs::create_dir_all(ws.join(".orchestrator")).unwrap();
    std::fs::write(ws.join("README.md"), "# Demo\n").unwrap();
    std::fs::write(ws.join(".orchestrator/fake-scenario"), "steps:1").unwrap();
    let sh = |c: &str| std::process::Command::new("sh").args(["-c", c]).current_dir(&ws).output().unwrap();
    sh("git init -q && git -c user.name=t -c user.email=t@t add -A && git -c user.name=t -c user.email=t@t commit -qm init && git config user.name t && git config user.email t@t");

    let core = Core::new(CoreDeps {
        data_dir: data.path().into(),
        catalog_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../catalog"),
        emitter: Arc::new(Quiet),
        demo_agents: true,
        notifier: Arc::new(|_, _| {}),
    })
    .unwrap();
    for a in core.refresh_agents(false).await {
        if !["fake-paid", "opencode"].contains(&a.id.as_str()) {
            core.db.set_agent_meta(&a.id, None, Some(false)).unwrap();
        }
    }
    let mut s = core.db.settings();
    s.planner_agent = Some("fake-paid".into());
    s.prefer_cloud_on_battery = false;
    s.prefer_cloud_when_hot = false;
    s.default_mode = "cost".into();
    s.executor_pool = vec![ExecutorEntry {
        agent_id: "opencode".into(),
        model: ModelRef {
            provider_id: "ollama".into(),
            provider_type: ProviderType::Ollama,
            name: model.clone(),
            display_name: None,
            base_url: Some("http://127.0.0.1:11434".into()),
            key_ref: None,
            tier: Tier::Local,
            mem_needed_gb: Some(2.9),
            ctx_len: Some(16384),
            price_in_per_m: None,
            price_out_per_m: None,
        },
        enabled: true,
    }];
    core.db.save_settings(&s).unwrap();
    let mut w = core.db.upsert_workspace(&ws.to_string_lossy(), "project", true).unwrap();
    w.settings.check_commands = Some(vec![]);
    core.db.update_workspace(&w).unwrap();

    let t = Instant::now();
    let id =
        core.start_run(StartOpts { workspace_id: w.id.clone(), goal: "Create the greeting module".into(), ..Default::default() }).unwrap();
    loop {
        for a in core.pending_approvals() {
            println!("approval: {} → stop", a.title);
            core.answer(&a.id, "stop").unwrap();
        }
        let run = core.db.run(&id).unwrap().unwrap();
        if RunStatus::parse(&run.status).is_some_and(|s| s.is_terminal()) && core.run_ctl(&id).is_none() {
            let steps = core.db.steps(&id).unwrap();
            for st in &steps {
                println!(
                    "step {} {} {} tier={:?} attempts={} reason={:?} detail={}",
                    st.idx, st.kind, st.status, st.tier, st.attempts, st.route_reason, st.detail
                );
            }
            println!(
                "run {} in {:.0}s · files: {:?}",
                run.status,
                t.elapsed().as_secs_f64(),
                std::fs::read_dir(&ws).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>()
            );
            let exec = steps.iter().find(|s| s.kind == "execute").unwrap();
            assert!(exec.attempts >= 1, "the local executor ran");
            // Either the model did the task, or the attempt stopped with a clear reason.
            assert!(exec.status == "passed" || exec.detail["agent_error"].as_str().is_some_and(|e| !e.is_empty()), "{exec:?}");
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(600), "timed out");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    core.unload_app_models().await;
    core.ollama.stop(&core.registry).await;
}
