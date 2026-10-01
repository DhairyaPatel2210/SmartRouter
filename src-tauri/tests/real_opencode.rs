//! Opt-in check of the real OpenCode CLI through the adapter, against the
//! fake OpenAI-compatible provider (no real model is called).
//! Run: PATH=/path/to/opencode/bin:$PATH cargo test --test real_opencode -- --ignored

use orchestrator_lib::adapters::{opencode::OpenCode, AdapterCtx, AgentAdapter, AgentEvent, StepRequest, StepResult};
use orchestrator_lib::providers::fake;
use orchestrator_lib::types::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn opencode_writes_a_file_via_fake_provider() {
    let srv = fake::start(0).await.unwrap();
    let ws = tempfile::tempdir().unwrap();
    let oc = OpenCode::new(AdapterCtx { registry: Default::default(), data_dir: ws.path().into(), demo_agents: false });
    assert!(oc.detect().await.installed, "opencode not on PATH");
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let model = ModelRef {
        provider_id: "fake-cloud".into(),
        provider_type: ProviderType::OpenaiCompatible,
        name: "fake-coder".into(),
        display_name: None,
        base_url: Some(srv.base_url()),
        key_ref: Some("unused".into()),
        tier: Tier::CheapCloud,
        mem_needed_gb: None,
        ctx_len: Some(32768),
        price_in_per_m: None,
        price_out_per_m: None,
    };
    let out = oc
        .run(
            StepRequest {
                run_id: "r".into(),
                step_id: "s".into(),
                workspace: ws.path().into(),
                task_file: ws.path().join("task.md"),
                prompt: "Write hello.txt containing a greeting".into(),
                model: Some(model),
                library_agent: None,
                timeout: Duration::from_secs(120),
                stall_timeout: Duration::from_secs(300),
                low_priority: true,
                api_key: Some(fake::FAKE_KEY.into()),
                cancel: Default::default(),
            },
            Arc::new(move |e| ev.lock().unwrap().push(e)),
        )
        .await
        .unwrap();
    let ev = events.lock().unwrap();
    assert_eq!(out.result, StepResult::Success, "{out:?} {ev:?}");
    assert!(ws.path().join("hello.txt").exists());
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Tokens { .. })));
}
