//! End-to-end runs driven by the scripted fake agent CLI (never a real paid
//! agent or cloud model). Each test gets its own data dir and workspace.

use orchestrator_lib::db::queries::{RunRow, StepRow};
use orchestrator_lib::engine::{Core, CoreDeps, StartOpts};
use orchestrator_lib::telemetry::{Emitter, UiEvent};
use orchestrator_lib::types::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Counter {
    logs: AtomicUsize,
    batches: AtomicUsize,
    max_batch: AtomicUsize,
}

impl Emitter for Counter {
    fn emit(&self, batch: Vec<UiEvent>) {
        self.batches.fetch_add(1, Ordering::SeqCst);
        self.max_batch.fetch_max(batch.len(), Ordering::SeqCst);
        let n = batch.iter().filter(|e| matches!(e, UiEvent::Log { .. })).count();
        self.logs.fetch_add(n, Ordering::SeqCst);
    }
}

fn model(tier: Tier, name: &str) -> ModelRef {
    ModelRef {
        provider_id: if tier == Tier::Local { "fake-local".into() } else { "fake-cloud".into() },
        provider_type: ProviderType::Fake,
        name: name.into(),
        display_name: None,
        base_url: None,
        key_ref: None,
        tier,
        mem_needed_gb: Some(0.1),
        ctx_len: None,
        price_in_per_m: Some(0.2),
        price_out_per_m: Some(0.6),
    }
}

struct Env {
    core: Arc<Core>,
    ws: PathBuf,
    ws_id: String,
    emitter: Arc<Counter>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn sh(dir: &Path, cmd: &str) -> String {
    let out = tokio::process::Command::new("sh").args(["-c", cmd]).current_dir(dir).output().await.unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

async fn setup(scenario: &str, git: bool, pool: Vec<ExecutorEntry>, mode: &str) -> Env {
    std::env::set_var("ORCH_FAKE_AGENT_BIN", env!("CARGO_BIN_EXE_orchestrator"));
    let data = tempfile::tempdir().unwrap();
    let wsd = tempfile::tempdir().unwrap();
    let ws = wsd.path().join("project");
    std::fs::create_dir_all(ws.join(".orchestrator")).unwrap();
    std::fs::write(ws.join("README.md"), "# Demo\n").unwrap();
    std::fs::write(ws.join(".orchestrator/fake-scenario"), scenario).unwrap();
    if git {
        sh(&ws, "git init -q && git -c user.name=t -c user.email=t@t add -A && git -c user.name=t -c user.email=t@t commit -qm init").await;
        sh(&ws, "git config user.name tester && git config user.email tester@example.com").await;
    }
    let emitter = Arc::new(Counter::default());
    let core = Core::new(CoreDeps {
        data_dir: data.path().to_path_buf(),
        catalog_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../catalog"),
        emitter: emitter.clone(),
        demo_agents: true,
        notifier: Arc::new(|_, _| {}),
    })
    .unwrap();
    let mut s = core.db.settings();
    s.planner_agent = Some("fake-paid".into());
    s.executor_pool = pool;
    s.prefer_cloud_on_battery = false;
    s.prefer_cloud_when_hot = false;
    s.cloud_notice_ack = vec!["fake-cloud".into()];
    s.default_mode = mode.into();
    core.db.save_settings(&s).unwrap();
    // Only the fake agents should count as installed for these tests.
    for a in core.refresh_agents(false).await.into_iter().filter(|a| !a.id.starts_with("fake")) {
        core.db.set_agent_meta(&a.id, None, Some(false)).unwrap();
    }
    let mut w = core.db.upsert_workspace(&ws.to_string_lossy(), "project", git).unwrap();
    w.settings.check_commands = Some(vec!["test ! -f fake_output/BROKEN".into()]);
    core.db.update_workspace(&w).unwrap();
    Env { core, ws, ws_id: w.id, emitter, _dirs: (data, wsd) }
}

fn local_pool() -> Vec<ExecutorEntry> {
    vec![
        ExecutorEntry { agent_id: "fake-oss".into(), model: model(Tier::Local, "fake-local-coder"), enabled: true },
        ExecutorEntry { agent_id: "fake-oss".into(), model: model(Tier::CheapCloud, "fake-cloud-coder"), enabled: true },
    ]
}

/// Waits for the run to end, answering approvals with `answer(kind) -> option`.
async fn wait(env: &Env, run_id: &str, answer: impl Fn(&str) -> &'static str) -> (RunRow, Vec<StepRow>) {
    let t = Instant::now();
    loop {
        for a in env.core.pending_approvals() {
            env.core.answer(&a.id, answer(&a.kind)).unwrap();
        }
        let run = env.core.db.run(run_id).unwrap().unwrap();
        if RunStatus::parse(&run.status).is_some_and(|s| s.is_terminal()) && env.core.run_ctl(run_id).is_none() {
            return (run, env.core.db.steps(run_id).unwrap());
        }
        assert!(t.elapsed() < Duration::from_secs(90), "run did not finish: {}", run.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn start(env: &Env, goal: &str) -> String {
    env.core.start_run(StartOpts { workspace_id: env.ws_id.clone(), goal: goal.into(), ..Default::default() }).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn balanced_run_plans_routes_verifies_commits_and_reviews() {
    let env = setup("pass", true, local_pool(), "balanced").await;
    let id = start(&env, "Add a greeting module with tests");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded", "{:?}", run.summary);
    let kinds: Vec<&str> = steps.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, vec!["plan", "execute", "execute", "execute", "review"]);
    // Plan step 2 is [high] → paid; 1 and 3 are [low] → local executor.
    let exec: Vec<&StepRow> = steps.iter().filter(|s| s.kind == "execute").collect();
    assert_eq!(exec[0].tier.as_deref(), Some("local"));
    assert_eq!(exec[1].tier.as_deref(), Some("premium"));
    assert_eq!(exec[1].agent_id.as_deref(), Some("fake-paid"));
    assert_eq!(exec[2].tier.as_deref(), Some("local"));
    assert_eq!(exec[2].library_agent_id.as_deref(), Some("test-writer"), "starter Library agent is honoured");
    assert!(exec.iter().all(|s| s.status == "passed" && s.commit_ref.is_some()));
    for n in 1..=3 {
        assert!(env.ws.join(format!("fake_output/step{n}.txt")).exists());
    }
    // One commit per step (+ library sync) on the working branch.
    let log = sh(&env.ws, "git log --format=%s").await;
    assert!(log.contains("step 1: Create the greeting module"), "{log}");
    assert!(log.contains("step 3:"), "{log}");
    assert!(run.branch.as_deref().unwrap().starts_with("orchestrator/"));
    assert_eq!(sh(&env.ws, "git rev-parse --abbrev-ref HEAD").await, run.branch.clone().unwrap());
    // Handoff files and Library sync.
    let h = env.ws.join(".orchestrator");
    assert!(h.join("plan.md").exists() && h.join("progress.md").exists() && h.join("tasks/3.md").exists());
    let task3 = std::fs::read_to_string(h.join("tasks/3.md")).unwrap();
    assert!(task3.contains("## Role") && task3.contains("Test writer"), "fallback role injection:\n{task3}");
    // Costs: local steps free, paid steps priced, savings recorded.
    assert_eq!(exec[0].cost_usd, 0.0);
    assert!(exec[1].cost_usd > 0.0);
    assert!(run.summary["saved_usd"].as_f64().unwrap() > 0.0);
    assert_eq!(run.summary["local_steps"], 2);
    assert!(run.library_snapshot_hash.is_some());
    assert!(env.emitter.logs.load(Ordering::SeqCst) > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_check_is_retried_on_same_executor() {
    let env = setup("break-check:1", true, local_pool(), "balanced").await;
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded");
    let s1 = steps.iter().find(|s| s.kind == "execute" && s.idx == 1).unwrap();
    assert_eq!(s1.attempts, 2);
    assert!(!s1.escalated);
    assert!(env.ws.join(".orchestrator/failures/1-1.md").exists());
    assert!(!env.ws.join("fake_output/BROKEN").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn escalation_ladder_local_cloud_paid_then_user_decides() {
    let env = setup("always-fail:1,steps:1", true, local_pool(), "balanced").await;
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |kind| if kind == "failed" { "skip" } else { "stop" }).await;
    let s1 = steps.iter().find(|s| s.kind == "execute").unwrap();
    // 2 local + 2 cheap cloud + 2 paid attempts.
    assert_eq!(s1.attempts, 6, "{s1:?}");
    assert!(s1.escalated);
    assert_eq!(s1.tier.as_deref(), Some("premium"));
    assert_eq!(s1.status, "skipped");
    assert_eq!(run.status, "failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn cost_mode_asks_before_escalating_to_paid() {
    let env = setup("always-fail:1,steps:1", true, local_pool(), "cost").await;
    let id = start(&env, "Do the thing");
    let asked = Arc::new(AtomicUsize::new(0));
    let a2 = asked.clone();
    let (_, steps) = wait(&env, &id, move |kind| {
        if kind == "escalate" {
            a2.fetch_add(1, Ordering::SeqCst);
            "skip"
        } else {
            "stop"
        }
    })
    .await;
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    let s1 = steps.iter().find(|s| s.kind == "execute").unwrap();
    assert_eq!(s1.attempts, 4, "local x2 + cloud x2, then ask before paid");
    assert!(steps.iter().all(|s| s.kind != "review"), "Cost mode has no review");
}

#[tokio::test(flavor = "multi_thread")]
async fn non_git_workspace_uses_snapshots_and_rolls_back() {
    let env = setup("pass", false, local_pool(), "cost").await;
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded");
    assert!(run.branch.is_none());
    assert!(env.ws.join("fake_output/step3.txt").exists());
    let s2 = steps.iter().find(|s| s.kind == "execute" && s.idx == 2).unwrap();
    assert_eq!(s2.commit_ref.as_deref(), Some("step-2"));
    let n = orchestrator_lib::engine::run::rollback(&env.core, &id, 2).await.unwrap();
    assert_eq!(n, 2);
    assert!(env.ws.join("fake_output/step1.txt").exists());
    assert!(!env.ws.join("fake_output/step2.txt").exists());
    assert!(!env.ws.join("fake_output/step3.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_pool_falls_through_when_local_is_unusable() {
    let mut pool = local_pool();
    pool[0].model.mem_needed_gb = Some(9_999.0); // can never fit the memory budget
    let env = setup("steps:1", true, pool, "cost").await;
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded");
    let s1 = steps.iter().find(|s| s.kind == "execute").unwrap();
    assert_eq!(s1.tier.as_deref(), Some("cheap_cloud"));
    assert!(s1.route_reason.as_deref().unwrap().contains("budget"), "{:?}", s1.route_reason);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_kills_the_running_agent() {
    let env = setup("slow:400,steps:2", true, local_pool(), "cost").await;
    let id = start(&env, "Do the thing");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    env.core.cancel_run(&id).unwrap();
    let (run, _) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "cancelled");
    assert!(env.core.registry.list().is_empty(), "no processes left: {:?}", env.core.registry.list());
}

#[tokio::test(flavor = "multi_thread")]
async fn streaming_10k_log_lines_stays_bounded_and_batched() {
    let env = setup("flood:10000,steps:1", true, local_pool(), "cost").await;
    let t = Instant::now();
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded");
    let s1 = steps.iter().find(|s| s.kind == "execute").unwrap();
    let mem = env.core.logs.get(&id, &s1.id, s1.idx, 0);
    assert!(mem.len() <= 2000, "ring buffer bounded, got {}", mem.len());
    let file = std::fs::read_to_string(env.core.logs.path(&id, s1.idx)).unwrap();
    assert!(file.lines().count() >= 10_000, "full log on disk");
    let batches = env.emitter.batches.load(Ordering::SeqCst);
    let logs = env.emitter.logs.load(Ordering::SeqCst);
    assert!(logs >= 10_000);
    assert!(batches < logs / 20, "UI events are batched: {batches} batches for {logs} lines");
    assert!(t.elapsed() < Duration::from_secs(30));
}

#[tokio::test(flavor = "multi_thread")]
async fn planner_printing_plan_to_stdout_still_works() {
    let env = setup("plan-stdout,steps:2", true, local_pool(), "cost").await;
    let id = start(&env, "Do the thing");
    let (run, steps) = wait(&env, &id, |_| "stop").await;
    assert_eq!(run.status, "succeeded");
    assert_eq!(steps.iter().filter(|s| s.kind == "execute").count(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn approve_before_paid_pauses_for_the_user() {
    let env = setup("steps:2", true, local_pool(), "balanced").await;
    let mut s = env.core.db.settings();
    s.approve_before_paid = true;
    env.core.db.save_settings(&s).unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let a2 = asked.clone();
    let id = start(&env, "Do the thing");
    let (run, _) = wait(&env, &id, move |kind| {
        if kind == "paid" {
            a2.fetch_add(1, Ordering::SeqCst);
            "run"
        } else {
            "stop"
        }
    })
    .await;
    assert_eq!(run.status, "succeeded");
    // planning + step 2 [high] + review
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_planning_restores_branch_and_stash() {
    let env = setup("always-fail:1,steps:1", true, vec![], "balanced").await;
    // Uncommitted work on a feature branch, like a real user.
    sh(&env.ws, "git switch -q -c feature && echo wip >> README.md && echo new > notes.txt").await;
    // No executor pool and a failing paid agent: step 1 fails everywhere and the user stops.
    let id = start(&env, "Do the thing");
    let (run, _) = wait(&env, &id, |_| "stop").await;
    assert_ne!(run.status, "succeeded");
    assert_eq!(sh(&env.ws, "git rev-parse --abbrev-ref HEAD").await, "feature");
    assert!(std::fs::read_to_string(env.ws.join("README.md")).unwrap().contains("wip"));
    assert!(env.ws.join("notes.txt").exists());
    assert_eq!(sh(&env.ws, "git stash list").await, "");
    assert_eq!(sh(&env.ws, "git branch --list 'orchestrator/*'").await, "");
    assert_eq!(run.summary["restored"], true);
}
