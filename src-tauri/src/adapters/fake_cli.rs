//! The scripted fake agent CLI (`<app> --fake-agent <prompt>`), used by
//! integration tests and demo mode. It behaves like a tiny coding agent:
//! plans, edits files in the workspace and prints JSONL events.
//!
//! Scenarios (env `FAKE_AGENT_SCENARIO`, comma-separated):
//! - `pass` (default)
//! - `fail-step:N`   step N exits non-zero on its first attempt
//! - `break-check:N` step N leaves `fake_output/BROKEN` on its first attempt
//! - `always-fail:N` step N always fails
//! - `flood:N`       print N log lines per step
//! - `slow:MS`       sleep MS between lines
//! - `plan-stdout`   print the plan instead of writing plan.md
//!
//! - `steps:N`       plan length (1..=6, default 3; also `FAKE_PLAN_STEPS`)
//!
//! A `<handoff>/fake-scenario` file in the workspace overrides the env var,
//! so parallel tests can use different scenarios.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PLAN_TASKS: &[&str] = &[
    "1. [low] Create the greeting module — reason: one new file with boilerplate\n   Files: fake_output/step1.txt\n   Check: `test -f fake_output/step1.txt`",
    "2. [high] Design the configuration loader — reason: architecture decision across modules\n   Files: fake_output/step2.txt, src/config.rs, src/main.rs, src/lib.rs",
    "3. [low] Write tests for the greeting module [agent: test-writer] — reason: tests for existing code\n   Files: fake_output/step3.txt",
    "4. [low] Update the README — reason: docs only\n   Files: fake_output/step4.txt",
    "5. [low] Format the code and write the commit message — reason: formatting\n   Files: fake_output/step5.txt",
    "6. [high] Fix the auth token refresh race — reason: concurrency and security\n   Files: fake_output/step6.txt",
];

struct Scenario {
    fail_step: Option<u32>,
    break_check: Option<u32>,
    always_fail: Option<u32>,
    flood: usize,
    slow_ms: u64,
    plan_stdout: bool,
    steps: Option<usize>,
}

/// Scenario from `<handoff>/fake-scenario` in the workspace (per-test), else the env var.
fn scenario_raw(cwd: &Path, handoff: &str) -> String {
    std::fs::read_to_string(cwd.join(handoff).join("fake-scenario"))
        .unwrap_or_else(|_| std::env::var("FAKE_AGENT_SCENARIO").unwrap_or_default())
}

fn scenario(raw: &str) -> Scenario {
    let mut s = Scenario { fail_step: None, break_check: None, always_fail: None, flood: 0, slow_ms: 0, plan_stdout: false, steps: None };
    for part in raw.split(',').map(str::trim) {
        let (k, v) = part.split_once(':').unwrap_or((part, ""));
        match k {
            "fail-step" => s.fail_step = v.parse().ok(),
            "break-check" => s.break_check = v.parse().ok(),
            "always-fail" => s.always_fail = v.parse().ok(),
            "flood" => s.flood = v.parse().unwrap_or(0),
            "slow" => s.slow_ms = v.parse().unwrap_or(0),
            "plan-stdout" => s.plan_stdout = true,
            "steps" => s.steps = v.parse().ok(),
            _ => {}
        }
    }
    s
}

fn emit(v: serde_json::Value, slow: u64) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
    if slow > 0 {
        std::thread::sleep(Duration::from_millis(slow));
    }
}

/// Entry point. Returns the process exit code.
pub fn main(args: &[String]) -> i32 {
    let prompt = args.iter().filter(|a| !a.starts_with("--")).cloned().collect::<Vec<_>>().join(" ");
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let handoff = std::env::var("FAKE_HANDOFF_DIR").unwrap_or_else(|_| crate::brand::HANDOFF_DIR_NAME.to_string());
    let sc = scenario(&scenario_raw(&cwd, &handoff));
    let model = std::env::var("FAKE_AGENT_MODEL").unwrap_or_else(|_| "fake-model".into());
    emit(serde_json::json!({"t": "text", "v": format!("fake agent started (model {model})")}), sc.slow_ms);

    if prompt.contains("## Planning task") {
        return plan(&cwd, &handoff, &prompt, &sc);
    }
    if prompt.contains("## Review task") {
        emit(serde_json::json!({"t": "tool", "name": "read", "path": format!("{handoff}/progress.md")}), sc.slow_ms);
        emit(serde_json::json!({"t": "text", "v": "Reviewed the diff. Looks good."}), sc.slow_ms);
        let _ = std::fs::write(cwd.join(&handoff).join("review.md"), "# Review\n\nLooks good. No blocking issues.\n");
        emit(serde_json::json!({"t": "tokens", "in": 1800, "out": 120}), sc.slow_ms);
        emit(serde_json::json!({"t": "result", "ok": true, "summary": "Review passed"}), 0);
        return 0;
    }

    let n = task_number(&prompt).unwrap_or(0);
    let counter = cwd.join(&handoff).join(".fake").join(format!("{n}.count"));
    let attempt = bump(&counter);
    emit(serde_json::json!({"t": "tool", "name": "read", "path": format!("{handoff}/tasks/{n}.md")}), sc.slow_ms);
    for i in 0..sc.flood {
        emit(serde_json::json!({"t": "text", "v": format!("working… line {i} of step {n}")}), 0);
    }
    if sc.always_fail == Some(n) || (sc.fail_step == Some(n) && attempt == 1) {
        emit(serde_json::json!({"t": "error", "v": format!("could not complete step {n} (scripted failure)")}), 0);
        return 1;
    }
    let out_dir = cwd.join("fake_output");
    let _ = std::fs::create_dir_all(&out_dir);
    let file = out_dir.join(format!("step{n}.txt"));
    let _ = std::fs::write(&file, format!("step {n} done by {model} (attempt {attempt})\n"));
    emit(serde_json::json!({"t": "tool", "name": "write", "path": format!("fake_output/step{n}.txt")}), sc.slow_ms);
    let broken = out_dir.join("BROKEN");
    if sc.break_check == Some(n) && attempt == 1 {
        let _ = std::fs::write(&broken, "scripted check failure\n");
        emit(serde_json::json!({"t": "tool", "name": "write", "path": "fake_output/BROKEN"}), sc.slow_ms);
    } else if broken.exists() {
        let _ = std::fs::remove_file(&broken);
    }
    emit(serde_json::json!({"t": "tokens", "in": 900 + n * 10, "out": 250}), sc.slow_ms);
    emit(serde_json::json!({"t": "result", "ok": true, "summary": format!("Completed step {n}")}), 0);
    0
}

fn plan(cwd: &Path, handoff: &str, prompt: &str, sc: &Scenario) -> i32 {
    let steps: usize = sc
        .steps
        .or_else(|| std::env::var("FAKE_PLAN_STEPS").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(3)
        .clamp(1, PLAN_TASKS.len());
    let goal = prompt
        .lines()
        .find_map(|l| l.strip_prefix("Goal: "))
        .unwrap_or("the goal")
        .to_string();
    let mut body = format!("# Plan\n\nGoal: {goal}\n\n");
    for t in &PLAN_TASKS[..steps] {
        body.push_str(t);
        body.push('\n');
    }
    emit(serde_json::json!({"t": "text", "v": "Reading the project…"}), sc.slow_ms);
    if sc.plan_stdout {
        for l in body.lines() {
            emit(serde_json::json!({"t": "text", "v": l}), 0);
        }
    } else {
        let dir = cwd.join(handoff);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("plan.md"), &body);
        emit(serde_json::json!({"t": "tool", "name": "write", "path": format!("{handoff}/plan.md")}), sc.slow_ms);
    }
    emit(serde_json::json!({"t": "tokens", "in": 2400, "out": 300}), sc.slow_ms);
    emit(serde_json::json!({"t": "result", "ok": true, "summary": format!("Wrote a {steps}-step plan")}), 0);
    0
}

fn task_number(prompt: &str) -> Option<u32> {
    let idx = prompt.find("tasks/")?;
    let rest = &prompt[idx + 6..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn bump(counter: &Path) -> u32 {
    let _ = std::fs::create_dir_all(counter.parent().unwrap_or(Path::new(".")));
    let n = std::fs::read_to_string(counter).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0) + 1;
    let _ = std::fs::write(counter, n.to_string());
    n
}
