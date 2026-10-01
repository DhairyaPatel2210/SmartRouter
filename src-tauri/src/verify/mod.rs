//! Verification gate: auto-detected check commands (tests, lint, types) run
//! after every step at the same lowered priority as agents.

use crate::proc::{self, CancelToken, ExitKind, ProcRegistry, RunOpts};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

const NPM_DEFAULT_TEST: &str = "no test specified";

/// Detects check commands from package.json, Cargo.toml, pyproject.toml, go.mod and Makefile.
pub fn detect_checks(ws: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(pkg) = std::fs::read_to_string(ws.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) {
            let pm = if ws.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if ws.join("yarn.lock").exists() {
                "yarn"
            } else if ws.join("bun.lockb").exists() || ws.join("bun.lock").exists() {
                "bun"
            } else {
                "npm"
            };
            let scripts = v.get("scripts").and_then(|s| s.as_object());
            let has = |k: &str| scripts.and_then(|s| s.get(k)).and_then(|x| x.as_str()).filter(|s| !s.contains(NPM_DEFAULT_TEST));
            for k in ["typecheck", "lint"] {
                if has(k).is_some() {
                    out.push(format!("{pm} run {k}"));
                }
            }
            if let Some(test) = has("test") {
                // Keep watch-mode runners from hanging the gate.
                let ci = if test.contains("vitest") && !test.contains("run") {
                    format!("{pm} test -- --run")
                } else if test.contains("jest") && !test.contains("--watch") {
                    format!("CI=1 {pm} test")
                } else {
                    format!("{pm} test")
                };
                out.push(ci);
            }
        }
    }
    if ws.join("Cargo.toml").exists() {
        out.push("cargo test --quiet".into());
    }
    let py = ws.join("pyproject.toml").exists() || ws.join("pytest.ini").exists() || ws.join("setup.cfg").exists();
    if py && (ws.join("tests").is_dir() || ws.join("test").is_dir()) {
        out.push(if ws.join("uv.lock").exists() { "uv run pytest -q".into() } else { "pytest -q".into() });
    }
    if ws.join("go.mod").exists() {
        out.push("go test ./...".into());
    }
    if let Ok(mk) = std::fs::read_to_string(ws.join("Makefile")) {
        if out.is_empty() && mk.lines().any(|l| l.starts_with("test:")) {
            out.push("make test".into());
        }
    }
    out
}

#[derive(Serialize, Clone, Debug)]
pub struct CheckResult {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Last lines of output (for the log and the failure file).
    pub tail: String,
    pub timed_out: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct GateResult {
    pub passed: bool,
    pub checks: Vec<CheckResult>,
    /// True when there was nothing to run (the step is "unverified").
    pub no_checks: bool,
}

pub async fn run_checks(
    ws: &Path,
    commands: &[String],
    registry: &ProcRegistry,
    owner: &str,
    cancel: &CancelToken,
    low_priority: bool,
    mut on_line: impl FnMut(&str, &str),
) -> GateResult {
    let mut checks = Vec::new();
    for c in commands {
        if cancel.is_cancelled() {
            break;
        }
        let t = Instant::now();
        let mut tail: VecDeque<String> = VecDeque::with_capacity(80);
        let mut cmd = proc::command("sh", low_priority);
        cmd.args(["-c", c]).current_dir(ws).env("CI", "1").env("FORCE_COLOR", "0").env("NO_COLOR", "1");
        let res =
            proc::run_streaming(cmd, RunOpts { registry, owner, label: "check", timeout: Duration::from_secs(600), cancel }, |_, l| {
                let l = crate::adapters::strip_ansi(l).to_string();
                on_line(c, &l);
                if tail.len() == 80 {
                    tail.pop_front();
                }
                tail.push_back(l);
            })
            .await;
        let (passed, code, timed_out) = match res {
            Ok(ExitKind::Exited(code)) => (code == 0, Some(code), false),
            Ok(ExitKind::TimedOut) => (false, None, true),
            Ok(_) => (false, None, false),
            Err(e) => {
                tail.push_back(format!("could not run: {e}"));
                (false, None, false)
            }
        };
        let r = CheckResult {
            command: c.clone(),
            passed,
            exit_code: code,
            duration_ms: t.elapsed().as_millis() as u64,
            tail: tail.into_iter().collect::<Vec<_>>().join("\n"),
            timed_out,
        };
        let stop = !r.passed;
        checks.push(r);
        if stop {
            break; // no point running lint after tests fail
        }
    }
    GateResult { passed: checks.iter().all(|c| c.passed), no_checks: commands.is_empty(), checks }
}

pub fn failure_report(step_title: &str, attempt: i64, gate: Option<&GateResult>, agent_error: Option<&str>, changed: &[String]) -> String {
    let mut s = format!("# Failure: {step_title} (attempt {attempt})\n\n");
    if let Some(e) = agent_error {
        s.push_str(&format!("## Agent error\n{e}\n\n"));
    }
    if let Some(g) = gate {
        for c in g.checks.iter().filter(|c| !c.passed) {
            s.push_str(&format!("## `{}` failed", c.command));
            if c.timed_out {
                s.push_str(" (timed out)");
            } else if let Some(code) = c.exit_code {
                s.push_str(&format!(" (exit {code})"));
            }
            s.push_str("\n```\n");
            let lines: Vec<&str> = c.tail.lines().collect();
            s.push_str(&lines[lines.len().saturating_sub(40)..].join("\n"));
            s.push_str("\n```\n\n");
        }
    }
    if !changed.is_empty() {
        s.push_str("## Files changed in the failed attempt (reverted)\n");
        for f in changed.iter().take(30) {
            s.push_str(&format!("- {f}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_node_and_rust_checks() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"test":"vitest","lint":"eslint .","build":"vite build"}}"#).unwrap();
        std::fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect_checks(d.path()), vec!["pnpm run lint", "pnpm test -- --run", "cargo test --quiet"]);
    }

    #[test]
    fn ignores_npm_placeholder_test() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#).unwrap();
        assert!(detect_checks(d.path()).is_empty());
    }

    #[tokio::test]
    async fn gate_passes_and_fails() {
        let d = tempfile::tempdir().unwrap();
        let reg = ProcRegistry::default();
        let c = CancelToken::default();
        let ok = run_checks(d.path(), &["true".into()], &reg, "s", &c, true, |_, _| {}).await;
        assert!(ok.passed);
        let bad = run_checks(d.path(), &["echo boom; exit 2".into(), "true".into()], &reg, "s", &c, true, |_, _| {}).await;
        assert!(!bad.passed);
        assert_eq!(bad.checks.len(), 1, "stops at the first failure");
        assert!(bad.checks[0].tail.contains("boom"));
        let report = failure_report("t", 1, Some(&bad), None, &["a.rs".into()]);
        assert!(report.contains("exit 2"));
    }
}
