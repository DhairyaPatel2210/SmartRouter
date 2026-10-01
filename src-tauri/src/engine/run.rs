//! The run lifecycle: prepare the workspace, sync the Library, plan, route
//! and execute each step through the verification gate with retries and
//! escalation, optional final review, and a summary with cost per tier.

use super::{ApprovalOption, ApprovalRequest, Assignment, Core, RunCtl, StartOpts};
use crate::adapters::{AgentEvent, StepRequest, StepResult};
use crate::brand;
use crate::db::now_ms;
use crate::db::queries::{RunRow, StepRow, WorkspaceRow};
use crate::handoff::{self, PlanTask};
use crate::library::{self, sync};
use crate::router::{self, RouteDecision, RouteEnv};
use crate::settings::{DirtyStrategy, Settings};
use crate::telemetry::{cost, UiEvent};
use crate::types::*;
use crate::verify;
use crate::workspace::{self, git, snapshot::SnapshotStore};
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// How the workspace is made reversible for this run.
enum Rewind {
    Git,
    Snapshots(SnapshotStore),
}

struct RunState {
    run: RunRow,
    ws_row: WorkspaceRow,
    ws: PathBuf,
    settings: Settings,
    rewind: Rewind,
    tasks: Vec<PlanTask>,
    steps: Vec<StepRow>,
    library: Vec<library::Item>,
    checks: Vec<String>,
    pool_override: Option<Vec<ExecutorEntry>>,
    /// Paid price used for "saved vs all-paid".
    paid_price: Option<AgentPrice>,
    paid_tokens: u64,
}

enum StepEnd {
    Passed,
    Accepted,
    Skipped,
    Stop,
    Cancelled,
}

pub async fn execute(core: Arc<Core>, ctl: Arc<RunCtl>, opts: StartOpts) {
    let slots = core.slots();
    let permit = match slots.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            core.bus.notice("info", "Queued: waiting for another run to finish (concurrency limit).", Some(&ctl.run_id));
            tokio::select! {
                p = slots.acquire_owned() => p.expect("semaphore"),
                _ = ctl.cancel.cancelled() => {
                    finish_cancelled_early(&core, &ctl);
                    return;
                }
            }
        }
    };
    core.sampler.acquire();
    let result = run_inner(&core, &ctl, &opts).await;
    core.sampler.release();
    drop(permit);

    let mut run = core.db.run(&ctl.run_id).ok().flatten().unwrap_or_default();
    let status = match &result {
        Ok(s) => *s,
        Err(_) if ctl.cancel.is_cancelled() => RunStatus::Cancelled,
        Err(_) => RunStatus::Failed,
    };
    if let Err(e) = &result {
        if !ctl.cancel.is_cancelled() {
            let text = format!("Run stopped: {e:#}");
            core.bus.notice("error", &text, Some(&ctl.run_id));
            core.db.writer().event(&ctl.run_id, None, "error", serde_json::json!({ "text": text }).to_string());
            run.summary["error"] = serde_json::json!(format!("{e:#}"));
        }
    }
    let steps = core.db.steps(&ctl.run_id).unwrap_or_default();
    for s in steps.iter().filter(|s| matches!(s.status.as_str(), "running" | "verifying" | "awaiting_approval" | "pending")) {
        let mut s = s.clone();
        s.status = StepStatus::Cancelled.to_string();
        let _ = core.db.save_step(&s);
        core.bus.send(UiEvent::Step { step: s });
    }
    let steps = core.db.steps(&ctl.run_id).unwrap_or_default();
    summarize(&mut run, &steps);
    run.status = status.to_string();
    run.ended_at = Some(now_ms());
    run.peak_mem_mb = run.peak_mem_mb.max(*ctl.peak_mb.lock());
    run.est_cost_usd = steps.iter().map(|s| s.cost_usd).sum();
    let _ = core.db.save_run(&run);
    core.db.flush();
    core.bus.send(UiEvent::Run { run: run.clone() });
    core.logs.drop_run(&steps.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
    core.forget_run(&ctl.run_id);

    // Nothing queued: unload what we loaded and stop the server we started.
    if core.active_runs().is_empty() {
        let freed = core.unload_app_models().await;
        if freed > 0.05 {
            core.bus.notice("info", format!("Run finished; unloaded local models ({freed:.1} GB freed)."), Some(&ctl.run_id));
        }
        let settings = core.db.settings();
        if core.ollama.managed() && !settings.ollama_always_on {
            core.ollama.stop(&core.registry).await;
        }
    }
    let saved = run.summary.get("saved_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let passed = run.summary.get("steps_passed").and_then(|v| v.as_u64()).unwrap_or(0);
    let total = run.summary.get("steps_total").and_then(|v| v.as_u64()).unwrap_or(0);
    core.notify(
        &format!("Run {}", status),
        &format!("{passed}/{total} steps passed · est. ${:.2} (saved ${saved:.2})", run.est_cost_usd),
    );
}

fn finish_cancelled_early(core: &Core, ctl: &RunCtl) {
    if let Ok(Some(mut run)) = core.db.run(&ctl.run_id) {
        run.status = RunStatus::Cancelled.to_string();
        run.ended_at = Some(now_ms());
        let _ = core.db.save_run(&run);
        core.bus.send(UiEvent::Run { run });
    }
    core.forget_run(&ctl.run_id);
}

fn summarize(run: &mut RunRow, steps: &[StepRow]) {
    let exec: Vec<&StepRow> = steps.iter().filter(|s| s.kind == "execute").collect();
    let count = |t: &str| steps.iter().filter(|s| s.tier.as_deref() == Some(t) && s.attempts > 0).count();
    let cost_tier = |t: &str| steps.iter().filter(|s| s.tier.as_deref() == Some(t)).map(|s| s.cost_usd).sum::<f64>();
    let saved: f64 = steps.iter().filter(|s| s.tier.as_deref() != Some("premium")).map(|s| (s.paid_equiv_usd - s.cost_usd).max(0.0)).sum();
    let s = &mut run.summary;
    s["steps_total"] = exec.len().into();
    s["steps_passed"] = exec.iter().filter(|x| x.status == "passed" || x.status == "accepted").count().into();
    s["steps_failed"] = exec.iter().filter(|x| x.status == "failed").count().into();
    s["premium_steps"] = count("premium").into();
    s["cheap_cloud_steps"] = count("cheap_cloud").into();
    s["local_steps"] = count("local").into();
    s["tokens_in"] = steps.iter().map(|x| x.tokens_in).sum::<i64>().into();
    s["tokens_out"] = steps.iter().map(|x| x.tokens_out).sum::<i64>().into();
    s["tokens_estimated"] = steps.iter().any(|x| x.tokens_estimated).into();
    s["cost_premium"] = cost_tier("premium").into();
    s["cost_cheap_cloud"] = cost_tier("cheap_cloud").into();
    s["cost_local"] = cost_tier("local").into();
    s["saved_usd"] = saved.into();
    s["escalations"] = exec.iter().filter(|x| x.escalated).count().into();
}

fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

fn save_step(core: &Core, s: &StepRow) {
    let _ = core.db.save_step(s);
    core.bus.send(UiEvent::Step { step: s.clone() });
}

fn save_run(core: &Core, r: &RunRow) {
    let _ = core.db.save_run(r);
    core.bus.send(UiEvent::Run { run: r.clone() });
}

/// Writes a line to a step's log (memory ring + file) and streams it to the UI.
fn log(core: &Core, run_id: &str, step_id: &str, kind: &str, text: &str) {
    let line = core.logs.push(step_id, kind, text);
    core.bus.send(UiEvent::Log { run_id: run_id.into(), step_id: step_id.into(), line });
}

/// A governor/route/check event: logged and stored as a normalized event.
fn event(core: &Core, run_id: &str, step_id: &str, kind: &str, text: &str) {
    log(core, run_id, step_id, kind, text);
    core.db.writer().event(run_id, Some(step_id), kind, serde_json::json!({ "text": text }).to_string());
}

fn model_id(m: &Option<ModelRef>) -> Option<String> {
    m.as_ref().map(|m| format!("{}/{}", m.provider_id, m.name))
}

fn agent_name(core: &Core, id: &str) -> String {
    core.agent_infos().into_iter().find(|a| a.id == id).map(|a| a.display_name).unwrap_or_else(|| id.to_string())
}

fn exec_label(core: &Core, d: &RouteDecision) -> String {
    let a = agent_name(core, &d.agent_id);
    match &d.model {
        Some(m) => format!("{a} · {}", m.label()),
        None => a,
    }
}

fn apply_decision(core: &Core, step: &mut StepRow, d: &RouteDecision) {
    step.agent_id = Some(d.agent_id.clone());
    step.model_id = model_id(&d.model);
    step.tier = Some(d.tier.to_string());
    step.route_reason = Some(d.reason.clone());
    step.detail["model"] = serde_json::to_value(&d.model).unwrap_or_default();
    step.detail["executor"] = serde_json::json!(exec_label(core, d));
}

fn decision_from_assignment(a: &Assignment, core: &Core) -> RouteDecision {
    let tier = a.model.as_ref().map(|m| m.tier).unwrap_or(Tier::Premium);
    let family = core.adapter(&a.agent_id).map(|x| x.family());
    RouteDecision {
        exec: if tier == Tier::Premium || family == Some(AgentFamily::Paid) && a.model.is_none() { Exec::Paid } else { Exec::Cheap },
        agent_id: a.agent_id.clone(),
        model: a.model.clone(),
        tier,
        reason: "reassigned by you".into(),
        fallthrough: vec![],
        needs_approval: false,
    }
}

async fn run_inner(core: &Arc<Core>, ctl: &Arc<RunCtl>, opts: &StartOpts) -> Result<RunStatus> {
    let settings = core.db.settings();
    core.logs.set_cap(settings.log_ring_lines);
    let ws_row = core.db.workspace(&ctl.workspace_id)?.ok_or_else(|| anyhow!("workspace not found"))?;
    let ws = PathBuf::from(&ws_row.path);
    if !ws.is_dir() {
        bail!("The folder {} no longer exists.", ws.display());
    }
    let mut run = core.db.run(&ctl.run_id)?.ok_or_else(|| anyhow!("run not found"))?;
    run.status = RunStatus::Planning.to_string();
    save_run(core, &run);

    // ---- 1. Make the workspace reversible ----
    let mut is_git = git::is_repo(&ws).await;
    if !is_git && opts.init_git {
        git::init(&ws).await?;
        is_git = true;
        core.bus.notice("info", "Initialised git so every step can be rolled back.", Some(&ctl.run_id));
    }
    let rewind = if is_git {
        let dirty = git::dirty_files(&ws).await.unwrap_or_default();
        if !dirty.is_empty() {
            match opts.dirty_strategy.or(ws_row.settings.dirty_strategy).unwrap_or_default() {
                DirtyStrategy::Stash => {
                    let msg = format!("{}: saved before run {}", brand::SHORT_NAME, short(&ctl.run_id));
                    git::stash(&ws, &msg).await?;
                    run.summary["stashed"] = serde_json::json!(msg);
                    core.bus.notice("info", format!("Stashed {} uncommitted change(s). Restore them from the run summary.", dirty.len()), Some(&ctl.run_id));
                }
                DirtyStrategy::Commit => {
                    git::commit_all(&ws, &format!("WIP: save work before {} run", brand::SHORT_NAME)).await?;
                }
                DirtyStrategy::RunOnTop => {}
            }
        }
        run.base_ref = git::current_branch(&ws).await;
        let branch = format!("{}/{}", brand::DATA_DIR_NAME, short(&ctl.run_id));
        git::start_branch(&ws, &branch).await?;
        run.branch = Some(branch);
        Rewind::Git
    } else {
        let store = SnapshotStore::new(&core.data_dir, &ctl.run_id);
        let ws2 = ws.clone();
        let store2 = SnapshotStore::new(&core.data_dir, &ctl.run_id);
        tokio::task::spawn_blocking(move || store2.take(&ws2, "base")).await??;
        Rewind::Snapshots(store)
    };
    handoff::reset_run_files(&ws)?;

    // ---- 2. Sync the Library into every installed CLI ----
    let items = library::list(&core.data_dir, Some(&ws));
    let profile = ws_row.library_profile_id.as_ref().and_then(|pid| core.db.profiles().ok()?.into_iter().find(|p| &p.0 == pid).map(|p| p.2));
    let resolved = library::resolve(&items, profile.as_deref());
    run.library_snapshot_hash = Some(library::snapshot_hash(&resolved));
    let targets = core.sync_targets();
    let target_refs: Vec<&str> = targets.iter().map(String::as_str).collect();
    let report = sync::apply(&ws, &sync::plan(&resolved, &target_refs), &target_refs, ws_row.settings.commit_generated)?;
    if !report.conflicts.is_empty() {
        let list: Vec<String> = report.conflicts.iter().map(|c| format!("{} ({})", c.path, c.reason)).collect();
        core.bus.notice("warn", format!("Library sync left {} file(s) untouched because you edited them: {}. Import them from the Library screen.", list.len(), list.join(", ")), Some(&ctl.run_id));
    }
    if matches!(rewind, Rewind::Git) {
        git::commit_all(&ws, &format!("chore: sync {} library", brand::SHORT_NAME)).await?;
        run.summary["base_commit"] = serde_json::json!(git::head(&ws).await);
    }
    save_run(core, &run);

    let checks = ws_row.settings.check_commands.clone().unwrap_or_else(|| verify::detect_checks(&ws));
    let mut st = RunState {
        run,
        ws_row,
        ws,
        settings,
        rewind,
        tasks: vec![],
        steps: vec![],
        library: resolved,
        checks,
        pool_override: opts.pool_override.clone(),
        paid_price: None,
        paid_tokens: 0,
    };

    // ---- 3. Plan ----
    plan(core, ctl, &mut st).await?;
    if ctl.cancel.is_cancelled() {
        return Ok(RunStatus::Cancelled);
    }
    st.run.status = RunStatus::Running.to_string();
    save_run(core, &st.run);

    // ---- 4. Execute ----
    let mut passed = 0;
    let mut all_ok = true;
    for i in 0..st.tasks.len() {
        ctl.wait_if_paused().await;
        if ctl.cancel.is_cancelled() {
            return Ok(RunStatus::Cancelled);
        }
        if !check_budget(core, ctl, &mut st).await {
            return Ok(RunStatus::Cancelled);
        }
        match execute_step(core, ctl, &mut st, i).await? {
            StepEnd::Passed | StepEnd::Accepted => passed += 1,
            StepEnd::Skipped => all_ok = false,
            StepEnd::Stop => {
                ctl.cancel.cancel();
                return Ok(RunStatus::Cancelled);
            }
            StepEnd::Cancelled => return Ok(RunStatus::Cancelled),
        }
    }

    // ---- 5. Final review ----
    let mode = st.settings.mode(&ctl.mode());
    if mode.review && passed > 0 && !ctl.cancel.is_cancelled() {
        review(core, ctl, &mut st).await?;
    }
    Ok(if all_ok { RunStatus::Succeeded } else { RunStatus::Failed })
}

// ------------------------------------------------------------------ planning

async fn plan(core: &Arc<Core>, ctl: &Arc<RunCtl>, st: &mut RunState) -> Result<()> {
    let mode = st.settings.mode(&ctl.mode());
    let env = core.build_env(&ctl.run_id, Some(&st.ws_row), &mode, &st.settings, st.pool_override.as_deref()).await;
    st.run.budget_planning = env.budget_planner.is_some();
    st.paid_price = env.paid_agent.as_ref().and_then(|a| st.settings.agent_prices.get(a).cloned());
    let decision = match env.paid_agent.is_some() || env.budget_planner.is_some() {
        true => env_paid(&env, "planning")?,
        false => {
            // No paid agent and no budget planner: let the best cheap executor plan.
            match router::route_step(
                &router::Classified { class: StepClass::Low, reason: "planning".into(), hard_rule: false },
                None,
                &RouteEnv { mode: ModeDef { low: Exec::Cheap, ..mode.clone() }, paid_agent: None, budget_planner: None, pool: env.pool.clone() },
            ) {
                Ok(mut d) => {
                    d.reason = "no paid agent: the cheap executor plans (turn on budget planning for a stronger planner)".into();
                    d
                }
                Err(e) => bail!(e),
            }
        }
    };
    let mut step = StepRow {
        id: uuid::Uuid::new_v4().to_string(),
        run_id: ctl.run_id.clone(),
        idx: 0,
        title: if st.run.budget_planning { "Plan the work (budget planning)".into() } else { "Plan the work".into() },
        class: StepClass::High.to_string(),
        kind: StepKind::Plan.to_string(),
        status: StepStatus::Pending.to_string(),
        ..Default::default()
    };
    apply_decision(core, &mut step, &decision);
    save_step(core, &step);
    st.steps.push(step.clone());

    let agents: Vec<handoff::AgentBrief> = st
        .library
        .iter()
        .filter(|i| i.kind == library::Kind::Agent)
        .map(|a| handoff::AgentBrief { id: a.id.clone(), description: a.description.clone(), tier: a.tier.clone().unwrap_or_else(|| "any".into()) })
        .collect();
    let hints = workspace_hints(&st.ws, &st.checks);
    let prompt = handoff::planning_prompt(&st.run.goal, mode.short_plan, &agents, &hints);
    let prompt_file = handoff::dir(&st.ws).join("tasks").join("plan-prompt.md");
    std::fs::write(&prompt_file, &prompt)?;

    if !approve_executor(core, ctl, st, &step, &decision).await? {
        bail!("Planning was not approved.");
    }
    let (outcome, text) = run_agent(core, ctl, st, &mut step, &decision, &prompt_file, prompt.clone(), None).await?;
    match outcome {
        Some(StepResult::Cancelled) | None => {
            step.status = StepStatus::Cancelled.to_string();
            save_step(core, &step);
            return Ok(());
        }
        _ => {}
    }
    let plan_md = std::fs::read_to_string(handoff::plan_path(&st.ws)).ok();
    let mut tasks = plan_md.as_deref().map(handoff::parse_plan).unwrap_or_default();
    if tasks.is_empty() {
        // Some agents print the plan instead of writing the file.
        tasks = handoff::parse_plan(&text);
        if !tasks.is_empty() {
            std::fs::write(handoff::plan_path(&st.ws), format!("# Plan\n\nGoal: {}\n\n{}", st.run.goal, text))?;
        }
    }
    if tasks.is_empty() {
        step.status = StepStatus::Failed.to_string();
        save_step(core, &step);
        bail!("The planner didn't produce a plan. Try rephrasing the goal or pick a different planner.");
    }
    step.status = StepStatus::Passed.to_string();
    step.ended_at = Some(now_ms());
    step.detail["tasks"] = tasks.len().into();
    save_step(core, &step);
    event(core, &ctl.run_id, &step.id, "route", &format!("Plan has {} step(s).", tasks.len()));

    // Create and pre-route every step so the user sees (and can change) assignments up front.
    for t in &tasks {
        let c = router::classify(t, &mode);
        let hint = t.agent.as_ref().and_then(|id| st.library.iter().find(|i| i.kind == library::Kind::Agent && &i.id == id)).and_then(|a| a.tier.clone());
        let mut s = StepRow {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: ctl.run_id.clone(),
            idx: t.n,
            title: t.title.clone(),
            class: c.class.to_string(),
            kind: StepKind::Execute.to_string(),
            library_agent_id: t.agent.clone().filter(|id| st.library.iter().any(|i| i.kind == library::Kind::Agent && &i.id == id)),
            status: StepStatus::Pending.to_string(),
            ..Default::default()
        };
        s.detail["class_reason"] = serde_json::json!(c.reason);
        s.detail["files"] = serde_json::json!(t.files);
        s.detail["check"] = serde_json::json!(t.check);
        match router::route_step(&c, hint.as_deref(), &env) {
            Ok(d) => apply_decision(core, &mut s, &d),
            Err(e) => s.route_reason = Some(e),
        }
        save_step(core, &s);
        st.steps.push(s);
    }
    st.tasks = tasks;
    Ok(())
}

fn env_paid(env: &RouteEnv, what: &str) -> Result<RouteDecision> {
    let c = router::Classified { class: StepClass::High, reason: what.into(), hard_rule: true };
    let paid_env = RouteEnv { mode: ModeDef { high: Exec::Paid, ..env.mode.clone() }, paid_agent: env.paid_agent.clone(), budget_planner: env.budget_planner.clone(), pool: vec![] };
    let mut d = router::route_step(&c, None, &paid_env).map_err(|e| anyhow!(e))?;
    d.reason = if env.budget_planner.is_some() { format!("{what} by the budget planner") } else { format!("{what} by the paid agent") };
    Ok(d)
}

fn workspace_hints(ws: &Path, checks: &[String]) -> String {
    let mut h = vec![];
    for (f, what) in [("package.json", "Node/TypeScript"), ("Cargo.toml", "Rust"), ("pyproject.toml", "Python"), ("go.mod", "Go")] {
        if ws.join(f).exists() {
            h.push(what.to_string());
        }
    }
    if !checks.is_empty() {
        h.push(format!("checks: {}", checks.join(", ")));
    }
    h.join("; ")
}

/// Re-routes pending steps after a mode change.
pub async fn reroute_pending(core: &Arc<Core>, ctl: &RunCtl) {
    let settings = core.db.settings();
    let mode = settings.mode(&ctl.mode());
    let ws = core.db.workspace(&ctl.workspace_id).ok().flatten();
    let env = core.build_env(&ctl.run_id, ws.as_ref(), &mode, &settings, None).await;
    let overrides = ctl.overrides.lock().clone();
    for mut s in core.db.steps(&ctl.run_id).unwrap_or_default() {
        if s.kind != "execute" || s.status != "pending" || overrides.contains_key(&s.id) {
            continue;
        }
        let t = PlanTask { title: s.title.clone(), files: serde_json::from_value(s.detail["files"].clone()).unwrap_or_default(), ..Default::default() };
        let c = router::classify(&t, &mode);
        s.class = c.class.to_string();
        if let Ok(d) = router::route_step(&c, None, &env) {
            apply_decision(core, &mut s, &d);
        }
        save_step(core, &s);
    }
}

// ------------------------------------------------------------------ budget

async fn check_budget(core: &Arc<Core>, ctl: &Arc<RunCtl>, st: &mut RunState) -> bool {
    let b = st.ws_row.settings.run_budget.clone().unwrap_or_else(|| st.settings.run_budget.clone());
    let spent: f64 = st.steps.iter().map(|s| s.cost_usd).sum();
    let done = st.steps.iter().filter(|s| s.kind == "execute" && s.attempts > 0).count() as u32;
    let reason = if b.max_cost_usd.is_some_and(|m| spent >= m) {
        Some(format!("estimated spend ${spent:.2} reached the cap of ${:.2}", b.max_cost_usd.unwrap()))
    } else if b.max_steps.is_some_and(|m| done >= m) {
        Some(format!("{done} steps reached the cap of {}", b.max_steps.unwrap()))
    } else if b.max_paid_tokens.is_some_and(|m| st.paid_tokens >= m) {
        Some(format!("{} paid tokens reached the cap of {}", st.paid_tokens, b.max_paid_tokens.unwrap()))
    } else {
        None
    };
    let Some(reason) = reason else { return true };
    let a = core
        .ask(
            ctl,
            ApprovalRequest {
                id: String::new(),
                run_id: String::new(),
                step_id: None,
                kind: "budget".into(),
                title: "Budget cap reached".into(),
                body: format!("This run paused because {reason}. Continue anyway?"),
                options: vec![ApprovalOption::new("continue", "Continue this run").primary(), ApprovalOption::new("stop", "Stop here").danger()],
            },
        )
        .await;
    if a == "continue" {
        // Lift the cap for the rest of this run.
        st.ws_row.settings.run_budget = Some(Default::default());
        st.settings.run_budget = Default::default();
        true
    } else {
        false
    }
}

// ------------------------------------------------------------------ approvals

/// Approve-before-paid / approve-before-cloud and the one-time cloud notice.
async fn approve_executor(core: &Arc<Core>, ctl: &Arc<RunCtl>, st: &mut RunState, step: &StepRow, d: &RouteDecision) -> Result<bool> {
    let approve_paid = st.ws_row.settings.approve_before_paid.unwrap_or(st.settings.approve_before_paid);
    let approve_cloud = st.ws_row.settings.approve_before_cloud.unwrap_or(st.settings.approve_before_cloud);
    let label = exec_label(core, d);
    if d.tier == Tier::CheapCloud {
        if let Some(m) = &d.model {
            let settings = core.db.settings();
            if m.provider_type != ProviderType::Fake && !settings.cloud_notice_ack.contains(&m.provider_id) {
                let a = core
                    .ask(
                        ctl,
                        ApprovalRequest {
                            id: String::new(),
                            run_id: String::new(),
                            step_id: Some(step.id.clone()),
                            kind: "cloud_notice".into(),
                            title: format!("Send code to {}?", m.provider_id),
                            body: format!(
                                "This step runs on {}. Code and files from \"{}\" will be sent to {} to do the work. You'll only be asked once per provider. Mark a workspace \"local only\" to keep it on this Mac.",
                                m.label(),
                                st.ws_row.display_name,
                                m.provider_id
                            ),
                            options: vec![ApprovalOption::new("allow", "Allow").primary(), ApprovalOption::new("stop", "Stop run").danger()],
                        },
                    )
                    .await;
                if a != "allow" {
                    return Ok(false);
                }
                let mut s = core.db.settings();
                s.cloud_notice_ack.push(m.provider_id.clone());
                core.db.save_settings(&s)?;
            }
        }
    }
    let needs = (d.tier == Tier::Premium && approve_paid) || (d.tier == Tier::CheapCloud && approve_cloud);
    if !needs {
        return Ok(true);
    }
    let what = if d.tier == Tier::Premium { "paid" } else { "cheap cloud" };
    let mut s = step.clone();
    s.status = StepStatus::AwaitingApproval.to_string();
    save_step(core, &s);
    let a = core
        .ask(
            ctl,
            ApprovalRequest {
                id: String::new(),
                run_id: String::new(),
                step_id: Some(step.id.clone()),
                kind: if d.tier == Tier::Premium { "paid".into() } else { "cloud".into() },
                title: format!("Run \"{}\" on {label}?", step.title),
                body: format!("You asked to approve every {what} call. {}", d.reason),
                options: vec![ApprovalOption::new("run", format!("Run on {label}")).primary(), ApprovalOption::new("stop", "Stop run").danger()],
            },
        )
        .await;
    Ok(a == "run")
}

// ------------------------------------------------------------------ agents

#[derive(Default)]
struct Acc {
    tin: u64,
    tout: u64,
    saw_tokens: bool,
    reported_cost: f64,
    stdout_chars: usize,
    text: String,
    edits: Vec<String>,
    outside: Vec<String>,
}

/// Runs one agent invocation for a step, streaming events. Returns the
/// result (None if it couldn't start) and the agent's text output.
#[allow(clippy::too_many_arguments)]
async fn run_agent(
    core: &Arc<Core>,
    ctl: &Arc<RunCtl>,
    st: &mut RunState,
    step: &mut StepRow,
    d: &RouteDecision,
    task_file: &Path,
    prompt: String,
    library_agent: Option<String>,
) -> Result<(Option<StepResult>, String)> {
    let adapter = core.adapter(&d.agent_id).ok_or_else(|| anyhow!("agent {} is not available", d.agent_id))?;
    let is_local = d.tier == Tier::Local;
    if is_local {
        prepare_local(core, ctl, step, d, &st.settings).await?;
    }
    let api_key = d.model.as_ref().and_then(|m| m.key_ref.as_ref()).and_then(|k| crate::providers::keychain::get(k));
    let step_cancel = ctl.cancel.child();
    *ctl.step_cancel.lock() = Some(step_cancel.clone());
    *ctl.interrupt.lock() = None;
    step.status = StepStatus::Running.to_string();
    step.started_at.get_or_insert(now_ms());
    step.attempts += 1;
    apply_decision(core, step, d);
    save_step(core, step);
    core.logs.open(&ctl.run_id, &step.id, step.idx);
    let label = exec_label(core, d);
    *ctl.label.lock() = Some(format!("{label} ({})", match d.tier {
        Tier::Local => "local",
        Tier::CheapCloud => "cheap cloud",
        Tier::Premium => "premium",
    }));
    event(core, &ctl.run_id, &step.id, "route", &format!("Attempt {} on {label}: {}", step.attempts, d.reason));
    if is_local {
        ctl.local_step.store(true, Ordering::SeqCst);
        core.governor.enter_local(&ctl.run_id);
    }

    let acc = Arc::new(Mutex::new(Acc::default()));
    let (c2, run_id, step_id, ws, acc2, ctl2) = (core.clone(), ctl.run_id.clone(), step.id.clone(), st.ws.clone(), acc.clone(), ctl.clone());
    let cost_mode = st.settings.mode(&ctl.mode()).high == Exec::Cheap;
    let tx: crate::adapters::EventTx = Arc::new(move |e: AgentEvent| {
        let w = c2.db.writer();
        match &e {
            AgentEvent::Stdout { text } => {
                {
                    let mut a = acc2.lock();
                    a.stdout_chars += text.len();
                    if a.text.len() < 64 * 1024 {
                        a.text.push_str(text);
                        a.text.push('\n');
                    }
                }
                log(&c2, &run_id, &step_id, "out", text);
            }
            AgentEvent::ToolCall { name, detail } => {
                log(&c2, &run_id, &step_id, "tool", &format!("{name} {detail}"));
                w.event(&run_id, Some(&step_id), "tool_call", serde_json::to_string(&e).unwrap_or_default());
            }
            AgentEvent::FileEdit { path } => {
                let inside = workspace::is_inside(&ws, path);
                let mut a = acc2.lock();
                a.edits.push(path.clone());
                if !inside {
                    a.outside.push(path.clone());
                }
                drop(a);
                w.event(&run_id, Some(&step_id), "file_edit", serde_json::to_string(&e).unwrap_or_default());
                if inside {
                    log(&c2, &run_id, &step_id, "edit", path);
                } else {
                    let msg = format!("Scope guard: the agent wrote outside the workspace: {path}");
                    log(&c2, &run_id, &step_id, "error", &msg);
                    c2.bus.notice("warn", &msg, Some(&run_id));
                    if cost_mode {
                        *ctl2.interrupt.lock() = Some(format!("scope:{path}"));
                        if let Some(c) = ctl2.step_cancel.lock().as_ref() {
                            c.cancel();
                        }
                    }
                }
            }
            AgentEvent::Tokens { input, output } => {
                let mut a = acc2.lock();
                a.tin += input;
                a.tout += output;
                a.saw_tokens = true;
                drop(a);
                w.event(&run_id, Some(&step_id), "tokens", serde_json::to_string(&e).unwrap_or_default());
            }
            AgentEvent::Cost { usd } => acc2.lock().reported_cost += usd,
            AgentEvent::Error { text } => {
                log(&c2, &run_id, &step_id, "error", text);
                w.event(&run_id, Some(&step_id), "error", serde_json::to_string(&e).unwrap_or_default());
            }
        }
    });

    let timeout = Duration::from_secs(st.settings.step_timeout_minutes.max(1) as u64 * 60);
    let req = StepRequest {
        run_id: ctl.run_id.clone(),
        step_id: step.id.clone(),
        workspace: st.ws.clone(),
        task_file: task_file.to_path_buf(),
        prompt: prompt.clone(),
        model: d.model.clone(),
        library_agent,
        timeout,
        low_priority: st.settings.lower_priority,
        api_key,
        cancel: step_cancel,
    };
    let res = adapter.run(req, tx).await;
    if is_local {
        ctl.local_step.store(false, Ordering::SeqCst);
        core.governor.leave_local(&ctl.run_id);
    }
    *ctl.label.lock() = None;
    core.logs.close(&step.id);

    // Tokens & cost (estimated from text length when the CLI doesn't report).
    let a = acc.lock();
    let (tin, tout, estimated) = if a.saw_tokens { (a.tin, a.tout, false) } else { (cost::estimate_tokens(prompt.len()), cost::estimate_tokens(a.stdout_chars), true) };
    let price = st.settings.agent_prices.get(&d.agent_id).cloned();
    let step_cost = if d.tier == Tier::Premium && a.reported_cost > 0.0 { a.reported_cost } else { cost::step_cost(d.tier, price.as_ref(), d.model.as_ref(), tin, tout) };
    step.tokens_in += tin as i64;
    step.tokens_out += tout as i64;
    step.tokens_estimated |= estimated;
    step.cost_usd += step_cost;
    step.paid_equiv_usd += if d.tier == Tier::Premium { step_cost } else { cost::paid_cost(st.paid_price.as_ref(), tin, tout) };
    if d.tier == Tier::Premium {
        st.paid_tokens += tin + tout;
    }
    *ctl.cost.lock() += step_cost;
    step.detail["edits"] = serde_json::json!(a.edits.iter().take(50).collect::<Vec<_>>());
    let text = a.text.clone();
    drop(a);
    save_step(core, step);

    match res {
        Ok(o) => {
            if let Some(e) = &o.error {
                step.detail["agent_error"] = serde_json::json!(e);
            }
            if let Some(s) = &o.summary {
                step.detail["summary"] = serde_json::json!(crate::adapters::truncate(s, 500));
            }
            Ok((Some(o.result), text))
        }
        Err(e) => {
            event(core, &ctl.run_id, &step.id, "error", &format!("Couldn't start {label}: {e:#}"));
            step.detail["agent_error"] = serde_json::json!(format!("{e:#}"));
            Ok((Some(StepResult::Failure), text))
        }
    }
}

/// Starts Ollama if needed, enforces one local model at a time, and records the load.
async fn prepare_local(core: &Arc<Core>, ctl: &Arc<RunCtl>, step: &StepRow, d: &RouteDecision, settings: &Settings) -> Result<()> {
    let Some(m) = &d.model else { return Ok(()) };
    if m.provider_type != ProviderType::Ollama {
        return Ok(());
    }
    let sys = crate::governor::sys_view();
    if core.ollama.ensure_running(settings, &core.registry, sys.total_gb).await? {
        event(core, &ctl.run_id, &step.id, "governor", "Started Ollama for this run (it stops when the run ends).");
    }
    let client = core.ollama.client();
    // One local model at a time: unload others first, and say so.
    for other in client.ps().await.unwrap_or_default() {
        if other.name != m.name {
            if client.unload(&other.name).await.is_ok() {
                core.governor.state.lock().loaded_by_app.remove(&other.name);
                event(
                    core,
                    &ctl.run_id,
                    &step.id,
                    "governor",
                    &format!("Unloaded {} ({:.1} GB) so only one local model is in memory.", other.name, other.size as f64 / 1_073_741_824.0),
                );
            }
        }
    }
    core.governor.mark_loaded(&m.name);
    Ok(())
}

// ------------------------------------------------------------------ steps

async fn rewind_to(st: &RunState, label: &str) -> Result<()> {
    match &st.rewind {
        Rewind::Git => git::reset_hard(&st.ws, label).await,
        Rewind::Snapshots(s) => s.restore(&st.ws, label).map(|_| ()),
    }
}

async fn changed_files(st: &RunState, base: &str) -> Vec<String> {
    match &st.rewind {
        Rewind::Git => git::changed_since(&st.ws, base).await.unwrap_or_default(),
        Rewind::Snapshots(s) => {
            let before = s.load(base).unwrap_or_default();
            let probe = format!("probe-{}", uuid::Uuid::new_v4().simple());
            let now = s.take(&st.ws, &probe).unwrap_or_default();
            SnapshotStore::changed(&before, &now)
        }
    }
}

async fn checkpoint(st: &RunState, n: i64, title: &str, agent: &str) -> Result<Option<String>> {
    match &st.rewind {
        Rewind::Git => git::commit_all(&st.ws, &format!("step {n}: {title}\n\nExecuted by {agent} via {}.", brand::PRODUCT_NAME)).await,
        Rewind::Snapshots(s) => {
            s.take(&st.ws, &format!("step-{n}"))?;
            Ok(Some(format!("step-{n}")))
        }
    }
}

async fn step_base(st: &RunState, idx: usize) -> String {
    match &st.rewind {
        Rewind::Git => git::head(&st.ws).await.unwrap_or_else(|| "HEAD".into()),
        Rewind::Snapshots(_) => {
            // Latest checkpoint before this step.
            st.steps
                .iter()
                .filter(|s| s.kind == "execute" && (s.idx as usize) < idx + 1 && s.commit_ref.is_some())
                .filter_map(|s| s.commit_ref.clone())
                .next_back()
                .unwrap_or_else(|| "base".into())
        }
    }
}

async fn execute_step(core: &Arc<Core>, ctl: &Arc<RunCtl>, st: &mut RunState, i: usize) -> Result<StepEnd> {
    let task = st.tasks[i].clone();
    let total = st.tasks.len();
    let step_pos = st.steps.iter().position(|s| s.kind == "execute" && s.idx == task.n).ok_or_else(|| anyhow!("step row missing"))?;
    let mut step = core.db.steps(&ctl.run_id)?.into_iter().find(|s| s.id == st.steps[step_pos].id).unwrap_or_else(|| st.steps[step_pos].clone());
    let base = step_base(st, i).await;
    let mut failure_notes: Option<String> = None;
    let mut on_executor = 0u32;

    let mode = st.settings.mode(&ctl.mode());
    let mut env = core.build_env(&ctl.run_id, Some(&st.ws_row), &mode, &st.settings, st.pool_override.as_deref()).await;
    let override_a = ctl.overrides.lock().get(&step.id).cloned();
    let library_agent = override_a.as_ref().and_then(|a| a.library_agent.clone()).or(step.library_agent_id.clone());
    let mut decision = match &override_a {
        Some(a) => decision_from_assignment(a, core),
        None => {
            let c = router::classify(&task, &mode);
            let hint = library_agent.as_ref().and_then(|id| st.library.iter().find(|x| x.kind == library::Kind::Agent && &x.id == id)).and_then(|a| a.tier.clone());
            match router::route_step(&c, hint.as_deref(), &env) {
                Ok(d) => d,
                Err(e) => {
                    step.status = StepStatus::Failed.to_string();
                    step.route_reason = Some(e.clone());
                    save_step(core, &step);
                    bail!(e);
                }
            }
        }
    };
    if decision.needs_approval {
        let a = core
            .ask(
                ctl,
                ApprovalRequest {
                    id: String::new(),
                    run_id: String::new(),
                    step_id: Some(step.id.clone()),
                    kind: "escalate".into(),
                    title: format!("Use the paid agent for \"{}\"?", task.title),
                    body: decision.reason.clone(),
                    options: vec![ApprovalOption::new("run", format!("Use {}", exec_label(core, &decision))).primary(), ApprovalOption::new("skip", "Skip step"), ApprovalOption::new("stop", "Stop run").danger()],
                },
            )
            .await;
        match a.as_str() {
            "run" => {}
            "skip" => return skip_step(core, st, step, &base).await,
            _ => return Ok(StepEnd::Stop),
        }
    }

    loop {
        if ctl.cancel.is_cancelled() {
            return Ok(StepEnd::Cancelled);
        }
        if !approve_executor(core, ctl, st, &step, &decision).await? {
            return Ok(StepEnd::Stop);
        }
        // Library role/rules/skills for CLIs without native support (fallback injection).
        let (role, rules, skills) = fallback_library(st, &decision.agent_id, library_agent.as_deref());
        let task_file = handoff::write_task(
            &st.ws,
            &handoff::TaskInput {
                n: task.n,
                total,
                goal: &st.run.goal,
                task: &task,
                plan_excerpt: handoff::plan_excerpt(&st.tasks, task.n),
                progress_tail: handoff::progress_tail(&st.ws, 5),
                checks: &st.checks,
                role,
                rules,
                skills,
                failure_notes: failure_notes.clone(),
            },
        )?;
        let prompt = handoff::execute_prompt(&st.ws, &task_file);
        let native_agent = library_agent.clone().filter(|_| sync::support(&decision.agent_id, library::Kind::Agent) == sync::Support::Native);
        on_executor += 1;
        let (result, _) = run_agent(core, ctl, st, &mut step, &decision, &task_file, prompt, native_agent).await?;

        // Interrupted by the governor (memory pressure) or the scope guard.
        if result == Some(StepResult::Cancelled) && !ctl.cancel.is_cancelled() {
            let why = ctl.interrupt.lock().take().unwrap_or_else(|| "interrupted".into());
            rewind_to(st, &base).await.ok();
            let cloud = env.pool.iter().find(|p| p.entry.model.tier == Tier::CheapCloud && p.blocked.is_none()).map(|p| p.entry.clone());
            let mut options = vec![];
            if let Some(c) = &cloud {
                options.push(ApprovalOption::new("cloud", format!("Continue on {}", c.model.label())).primary());
            }
            options.push(ApprovalOption::new("retry", "Retry here"));
            options.push(ApprovalOption::new("skip", "Skip step"));
            options.push(ApprovalOption::new("stop", "Stop run").danger());
            let (title, body) = if why.starts_with("scope:") {
                (format!("Paused step {}: wrote outside the workspace", task.n), format!("The agent touched {}. The step's changes were rolled back.", &why[6..]))
            } else {
                (format!("Paused step {}: {why}", task.n), "The local model was unloaded to keep your Mac responsive. The step's partial changes were rolled back.".to_string())
            };
            event(core, &ctl.run_id, &step.id, "governor", &title);
            step.status = StepStatus::AwaitingApproval.to_string();
            save_step(core, &step);
            let a = core.ask(ctl, ApprovalRequest { id: String::new(), run_id: String::new(), step_id: Some(step.id.clone()), kind: if why.starts_with("scope:") { "scope".into() } else { "pressure".into() }, title, body, options }).await;
            match a.as_str() {
                "cloud" => {
                    let c = cloud.unwrap();
                    decision = RouteDecision { exec: Exec::Cheap, agent_id: c.agent_id.clone(), tier: c.model.tier, model: Some(c.model), reason: "moved to cloud after the local step was paused".into(), fallthrough: vec![], needs_approval: false };
                    on_executor = 0;
                    continue;
                }
                "retry" => {
                    env = core.build_env(&ctl.run_id, Some(&st.ws_row), &mode, &st.settings, st.pool_override.as_deref()).await;
                    on_executor = 0;
                    continue;
                }
                "skip" => return skip_step(core, st, step, &base).await,
                _ => return Ok(StepEnd::Stop),
            }
        }
        if result.is_none() || result == Some(StepResult::Cancelled) {
            return Ok(StepEnd::Cancelled);
        }

        // ---- Verification gate ----
        let agent_ok = result == Some(StepResult::Success);
        let mut checks = st.checks.clone();
        if let Some(c) = &task.check {
            if !checks.contains(c) {
                checks.insert(0, c.clone());
            }
        }
        let gate = if agent_ok {
            step.status = StepStatus::Verifying.to_string();
            save_step(core, &step);
            let (c2, rid, sid) = (core.clone(), ctl.run_id.clone(), step.id.clone());
            let g = verify::run_checks(&st.ws, &checks, &core.registry, &step.id, &ctl.cancel, st.settings.lower_priority, move |cmd, line| {
                let _ = cmd;
                log(&c2, &rid, &sid, "check", line);
            })
            .await;
            for c in &g.checks {
                event(core, &ctl.run_id, &step.id, "check", &format!("{} `{}` ({:.1}s)", if c.passed { "✓" } else { "✗" }, c.command, c.duration_ms as f64 / 1000.0));
            }
            if g.no_checks {
                event(core, &ctl.run_id, &step.id, "check", "No check commands configured; step is unverified. Add checks in Settings → Workspace.");
            }
            Some(g)
        } else {
            None
        };
        if ctl.cancel.is_cancelled() {
            return Ok(StepEnd::Cancelled);
        }
        let passed = agent_ok && gate.as_ref().is_some_and(|g| g.passed);
        let changed = changed_files(st, &base).await;

        if passed {
            if changed.is_empty() {
                event(core, &ctl.run_id, &step.id, "info", "The agent made no file changes in this step.");
            }
            let agent = exec_label(core, &decision);
            step.commit_ref = checkpoint(st, task.n, &task.title, &agent).await?;
            step.status = StepStatus::Passed.to_string();
            step.ended_at = Some(now_ms());
            step.detail["changed"] = serde_json::json!(changed.iter().take(50).collect::<Vec<_>>());
            step.detail["unverified"] = gate.as_ref().is_some_and(|g| g.no_checks).into();
            save_step(core, &step);
            let files = if changed.is_empty() { "no file changes".to_string() } else { changed.iter().take(8).cloned().collect::<Vec<_>>().join(", ") };
            handoff::append_progress(&st.ws, &format!("- Step {}: {} — {} ({}). Changed: {files}", task.n, task.title, agent, decision.tier))?;
            st.steps[step_pos] = step;
            return Ok(StepEnd::Passed);
        }

        // ---- Failure: retry, escalate or ask ----
        let agent_error = step.detail.get("agent_error").and_then(|v| v.as_str()).map(String::from).or_else(|| {
            if !agent_ok {
                Some(format!("{} exited without finishing the task.", exec_label(core, &decision)))
            } else {
                None
            }
        });
        let report = verify::failure_report(&task.title, step.attempts, gate.as_ref(), agent_error.as_deref(), &changed);
        handoff::write_failure(&st.ws, task.n, step.attempts, &report)?;
        failure_notes = Some(report);
        event(core, &ctl.run_id, &step.id, "error", &format!("Attempt {} failed{}.", step.attempts, if agent_ok { " verification" } else { "" }));

        if on_executor < mode.attempts_per_executor {
            rewind_to(st, &base).await?;
            event(core, &ctl.run_id, &step.id, "route", "Retrying on the same executor with the failure notes.");
            continue;
        }
        env = core.build_env(&ctl.run_id, Some(&st.ws_row), &mode, &st.settings, st.pool_override.as_deref()).await;
        if let Some(next) = router::escalate(&decision, &env) {
            let go = if next.needs_approval {
                let a = core
                    .ask(
                        ctl,
                        ApprovalRequest {
                            id: String::new(),
                            run_id: String::new(),
                            step_id: Some(step.id.clone()),
                            kind: "escalate".into(),
                            title: format!("Step {} failed {} times. Escalate to {}?", task.n, step.attempts, exec_label(core, &next)),
                            body: "Cost mode asks before using the paid agent. You can also keep the failed changes, skip the step or stop.".into(),
                            options: vec![
                                ApprovalOption::new("escalate", format!("Escalate to {}", exec_label(core, &next))).primary(),
                                ApprovalOption::new("accept", "Keep changes anyway"),
                                ApprovalOption::new("skip", "Skip step"),
                                ApprovalOption::new("stop", "Stop run").danger(),
                            ],
                        },
                    )
                    .await;
                match a.as_str() {
                    "escalate" => true,
                    "accept" => return accept_step(core, st, step, step_pos, &task, &decision).await,
                    "skip" => return skip_step(core, st, step, &base).await,
                    _ => {
                        rewind_to(st, &base).await.ok();
                        return Ok(StepEnd::Stop);
                    }
                }
            } else {
                true
            };
            if go {
                rewind_to(st, &base).await?;
                event(core, &ctl.run_id, &step.id, "route", &next.reason);
                step.escalated = true;
                decision = next;
                on_executor = 0;
                continue;
            }
        }

        // Nowhere left to go: the user decides. The failed changes stay in
        // the tree so "keep changes" can accept them.
        step.status = StepStatus::Failed.to_string();
        save_step(core, &step);
        let a = core
            .ask(
                ctl,
                ApprovalRequest {
                    id: String::new(),
                    run_id: String::new(),
                    step_id: Some(step.id.clone()),
                    kind: "failed".into(),
                    title: format!("Step {} failed: {}", task.n, task.title),
                    body: "Checks didn't pass after retries and escalation. See the log for details.".into(),
                    options: vec![
                        ApprovalOption::new("retry", "Retry").primary(),
                        ApprovalOption::new("accept", "Keep changes anyway"),
                        ApprovalOption::new("skip", "Skip step"),
                        ApprovalOption::new("stop", "Stop run").danger(),
                    ],
                },
            )
            .await;
        match a.as_str() {
            "retry" => {
                rewind_to(st, &base).await?;
                on_executor = 0;
                continue;
            }
            "accept" => return accept_step(core, st, step, step_pos, &task, &decision).await,
            "skip" => return skip_step(core, st, step, &base).await,
            _ => {
                rewind_to(st, &base).await.ok();
                return Ok(StepEnd::Stop);
            }
        }
    }
}

async fn accept_step(core: &Arc<Core>, st: &mut RunState, mut step: StepRow, pos: usize, task: &PlanTask, d: &RouteDecision) -> Result<StepEnd> {
    let agent = exec_label(core, d);
    step.commit_ref = checkpoint(st, task.n, &format!("{} (accepted with failing checks)", task.title), &agent).await?;
    step.status = StepStatus::Accepted.to_string();
    step.ended_at = Some(now_ms());
    save_step(core, &step);
    handoff::append_progress(&st.ws, &format!("- Step {}: {} — accepted by you with failing checks ({agent}).", task.n, task.title))?;
    st.steps[pos] = step;
    Ok(StepEnd::Accepted)
}

async fn skip_step(core: &Arc<Core>, st: &mut RunState, mut step: StepRow, base: &str) -> Result<StepEnd> {
    rewind_to(st, base).await.ok();
    step.status = StepStatus::Skipped.to_string();
    step.ended_at = Some(now_ms());
    save_step(core, &step);
    handoff::append_progress(&st.ws, &format!("- Step {}: {} — skipped.", step.idx, step.title))?;
    Ok(StepEnd::Skipped)
}

/// Role, rules and skills to inject into the task file for CLIs that can't
/// receive them natively.
fn fallback_library(st: &RunState, cli: &str, library_agent: Option<&str>) -> (Option<String>, Vec<String>, Vec<(String, String, String)>) {
    let mut role = None;
    if let Some(id) = library_agent {
        if let Some(a) = st.library.iter().find(|i| i.kind == library::Kind::Agent && i.id == id) {
            // Every CLI gets the role in the task file so behaviour matches.
            role = Some(format!("You are acting as \"{}\".\n{}", a.display_name, sync::agent_body(a, &st.library)));
        }
    }
    let rules = if sync::support(cli, library::Kind::Rule) == sync::Support::Fallback {
        st.library.iter().filter(|i| i.kind == library::Kind::Rule).map(|r| format!("- **{}**: {}", r.display_name, r.body.trim())).collect()
    } else {
        vec![]
    };
    let skills = if sync::support(cli, library::Kind::Skill) == sync::Support::Fallback {
        st.library
            .iter()
            .filter(|i| i.kind == library::Kind::Skill)
            .map(|s| (s.id.clone(), s.description.clone(), format!("{}/skills/{}/SKILL.md", brand::HANDOFF_DIR_NAME, s.id)))
            .collect()
    } else {
        vec![]
    };
    (role, rules, skills)
}

// ------------------------------------------------------------------ review

async fn review(core: &Arc<Core>, ctl: &Arc<RunCtl>, st: &mut RunState) -> Result<()> {
    st.run.status = RunStatus::Reviewing.to_string();
    save_run(core, &st.run);
    let mode = st.settings.mode(&ctl.mode());
    let env = core.build_env(&ctl.run_id, Some(&st.ws_row), &mode, &st.settings, st.pool_override.as_deref()).await;
    let Ok(decision) = env_paid(&env, "final review") else {
        core.bus.notice("info", "Skipped the final review: no paid agent or budget planner available.", Some(&ctl.run_id));
        return Ok(());
    };
    let mut step = StepRow {
        id: uuid::Uuid::new_v4().to_string(),
        run_id: ctl.run_id.clone(),
        idx: st.tasks.len() as i64 + 1,
        title: "Final review".into(),
        class: StepClass::High.to_string(),
        kind: StepKind::Review.to_string(),
        status: StepStatus::Pending.to_string(),
        ..Default::default()
    };
    apply_decision(core, &mut step, &decision);
    save_step(core, &step);
    if !approve_executor(core, ctl, st, &step, &decision).await? {
        step.status = StepStatus::Skipped.to_string();
        save_step(core, &step);
        return Ok(());
    }
    let base_commit = st.run.summary.get("base_commit").and_then(|v| v.as_str()).map(String::from);
    let prompt = handoff::review_prompt(&st.run.goal, base_commit.as_deref());
    let prompt_file = handoff::dir(&st.ws).join("tasks").join("review-prompt.md");
    std::fs::write(&prompt_file, &prompt)?;
    let base = step_base(st, st.tasks.len()).await;
    let (result, text) = run_agent(core, ctl, st, &mut step, &decision, &prompt_file, prompt, None).await?;
    if result == Some(StepResult::Cancelled) || ctl.cancel.is_cancelled() {
        step.status = StepStatus::Cancelled.to_string();
        save_step(core, &step);
        return Ok(());
    }
    let review = std::fs::read_to_string(handoff::dir(&st.ws).join("review.md")).unwrap_or(text);
    let verdict = review.lines().find(|l| l.to_lowercase().starts_with("verdict")).unwrap_or("Verdict: see review").trim().to_string();
    step.detail["verdict"] = serde_json::json!(verdict);
    step.detail["review"] = serde_json::json!(crate::adapters::truncate(&review, 4000));
    // Keep reviewer fixes only if checks still pass.
    let changed = changed_files(st, &base).await;
    if !changed.is_empty() {
        let g = verify::run_checks(&st.ws, &st.checks, &core.registry, &step.id, &ctl.cancel, st.settings.lower_priority, |_, _| {}).await;
        if g.passed {
            step.commit_ref = checkpoint(st, step.idx, "review fixes", &exec_label(core, &decision)).await?;
            event(core, &ctl.run_id, &step.id, "check", &format!("Kept the reviewer's fixes to {} file(s); checks pass.", changed.len()));
        } else {
            rewind_to(st, &base).await?;
            event(core, &ctl.run_id, &step.id, "check", "Reverted the reviewer's edits because checks failed.");
        }
    }
    step.status = if result == Some(StepResult::Success) { StepStatus::Passed } else { StepStatus::Failed }.to_string();
    step.ended_at = Some(now_ms());
    save_step(core, &step);
    st.run.summary["review_verdict"] = serde_json::json!(verdict);
    save_run(core, &st.run);
    Ok(())
}

// ------------------------------------------------------------------ after a run

/// Rolls a finished (or paused) run back to just before step `idx`.
pub async fn rollback(core: &Arc<Core>, run_id: &str, idx: i64) -> Result<usize> {
    if core.run_ctl(run_id).is_some_and(|c| !c.paused.load(Ordering::SeqCst)) {
        bail!("Pause or stop the run before rolling back.");
    }
    let run = core.db.run(run_id)?.ok_or_else(|| anyhow!("run not found"))?;
    let ws = core.db.workspace(&run.workspace_id)?.ok_or_else(|| anyhow!("workspace not found"))?;
    let ws_path = PathBuf::from(&ws.path);
    let steps = core.db.steps(run_id)?;
    let prev = steps.iter().filter(|s| s.idx < idx && s.commit_ref.is_some() && s.kind != "plan").filter_map(|s| s.commit_ref.clone()).next_back();
    if let Some(branch) = &run.branch {
        if git::current_branch(&ws_path).await.as_deref() != Some(branch) {
            bail!("Switch back to {branch} to roll back this run.");
        }
        let target = prev.or_else(|| run.summary.get("base_commit").and_then(|v| v.as_str()).map(String::from)).ok_or_else(|| anyhow!("no commit to roll back to"))?;
        git::reset_hard(&ws_path, &target).await?;
    } else {
        let store = SnapshotStore::new(&core.data_dir, run_id);
        store.restore(&ws_path, &prev.unwrap_or_else(|| "base".into()))?;
    }
    let mut n = 0;
    for mut s in steps.into_iter().filter(|s| s.idx >= idx && s.kind != "plan") {
        if matches!(s.status.as_str(), "passed" | "accepted") {
            s.status = StepStatus::RolledBack.to_string();
            s.commit_ref = None;
            core.db.save_step(&s)?;
            core.bus.send(UiEvent::Step { step: s });
            n += 1;
        }
    }
    core.bus.notice("info", format!("Rolled back {n} step(s)."), Some(run_id));
    Ok(n)
}

/// Accepts a finished run: prunes snapshots, optionally merges the working
/// branch into the original branch and restores stashed changes.
pub async fn accept(core: &Arc<Core>, run_id: &str, merge: bool, restore_stash: bool) -> Result<String> {
    let mut run = core.db.run(run_id)?.ok_or_else(|| anyhow!("run not found"))?;
    if core.run_ctl(run_id).is_some() {
        bail!("The run is still active.");
    }
    let ws = core.db.workspace(&run.workspace_id)?.ok_or_else(|| anyhow!("workspace not found"))?;
    let ws_path = PathBuf::from(&ws.path);
    SnapshotStore::new(&core.data_dir, run_id).prune();
    let mut msg = vec!["Run accepted.".to_string()];
    if let (Some(branch), Some(base)) = (run.branch.clone(), run.base_ref.clone()) {
        if merge {
            git::switch(&ws_path, &base).await?;
            let r = crate::proc::output("git", &["merge", "--no-ff", "-m", &format!("Merge {branch} ({})", brand::PRODUCT_NAME), &branch], Some(&ws_path), Duration::from_secs(60)).await?;
            if r.0 != 0 {
                let _ = crate::proc::output("git", &["merge", "--abort"], Some(&ws_path), Duration::from_secs(20)).await;
                git::switch(&ws_path, &branch).await.ok();
                bail!("Merge into {base} had conflicts, so nothing was merged. Merge {branch} yourself.");
            }
            msg.push(format!("Merged {branch} into {base}."));
        }
        if restore_stash && run.summary.get("stashed").is_some() {
            if !merge {
                git::switch(&ws_path, &base).await?;
            }
            let r = crate::proc::output("git", &["stash", "pop"], Some(&ws_path), Duration::from_secs(30)).await?;
            if r.0 == 0 {
                msg.push("Restored your stashed changes.".into());
                run.summary.as_object_mut().map(|o| o.remove("stashed"));
            } else {
                msg.push("Couldn't restore the stash automatically; run `git stash pop` yourself.".into());
            }
        }
    }
    run.summary["accepted"] = true.into();
    core.db.save_run(&run)?;
    core.bus.send(UiEvent::Run { run });
    Ok(msg.join(" "))
}
