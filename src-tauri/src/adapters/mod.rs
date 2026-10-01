//! Agent adapter layer: every coding-agent CLI sits behind [`AgentAdapter`],
//! so the router never knows which CLI it is driving.
//!
//! Most adapters are "spawn a headless CLI and parse its stream"; they
//! implement the small [`CliSpec`] trait and get `run` for free via
//! [`run_cli`].

pub mod aider;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod fake;
pub mod fake_cli;
pub mod goose;
pub mod opencode;

use crate::proc::{self, CancelToken, ExitKind, ProcRegistry, RunOpts, Stream};
use crate::types::*;
use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

/// Normalized events every adapter emits.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Stdout {
        text: String,
    },
    ToolCall {
        name: String,
        detail: String,
    },
    FileEdit {
        path: String,
    },
    Tokens {
        input: u64,
        output: u64,
    },
    /// Cost reported by the CLI itself (e.g. Claude Code's `total_cost_usd`).
    Cost {
        usd: f64,
    },
    Error {
        text: String,
    },
}

pub type EventTx = Arc<dyn Fn(AgentEvent) + Send + Sync>;
pub type ProgressTx = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Default)]
pub struct DetectResult {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthStatus {
    Ok,
    Missing { hint: String },
    Unknown,
}

pub struct Workspace {
    pub path: PathBuf,
}

pub struct StepRequest {
    pub run_id: String,
    pub step_id: String,
    pub workspace: PathBuf,
    /// Absolute path of `.orchestrator/tasks/<n>.md` (or plan/review prompt file).
    pub task_file: PathBuf,
    /// The full instruction given to the agent (includes the task file text,
    /// so small local models don't need to go looking for it).
    pub prompt: String,
    pub model: Option<ModelRef>,
    pub library_agent: Option<String>,
    pub timeout: Duration,
    pub low_priority: bool,
    /// Read from the Keychain at spawn time; reaches the CLI only via env.
    pub api_key: Option<String>,
    pub cancel: CancelToken,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepResult {
    Success,
    Failure,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepOutcome {
    pub result: StepResult,
    pub exit_code: Option<i32>,
    /// The agent's final message, if it gave one.
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn family(&self) -> AgentFamily;
    /// Model backends it can drive. Empty = only its own built-in models.
    fn supported_providers(&self) -> &[ProviderType];
    async fn detect(&self) -> DetectResult;
    async fn check_auth(&self) -> AuthStatus;
    /// Installation goes through the catalog installer (user sees and confirms
    /// every command); adapters only describe what to run.
    async fn install(&self, tx: ProgressTx) -> Result<()> {
        tx(format!("Install {} from Agents & Models", self.display_name()));
        anyhow::bail!("use the catalog installer")
    }
    async fn configure_model(&self, _ws: &Workspace, _model: &ModelRef) -> Result<()> {
        Ok(())
    }
    /// The CLI description for headless adapters (enables the default `run`).
    fn cli_spec(&self) -> Option<&dyn CliSpec> {
        None
    }
    /// Registry of processes this adapter started.
    fn procs(&self) -> &ProcRegistry;
    async fn run(&self, req: StepRequest, tx: EventTx) -> Result<StepOutcome> {
        let spec = self.cli_spec().ok_or_else(|| anyhow::anyhow!("{} has no runner", self.id()))?;
        let label = self.display_name().to_string();
        run_cli(spec, self.procs(), &label, req, tx).await
    }
    async fn cancel(&self, step_id: &str) -> Result<()> {
        self.procs().kill_owner(step_id).await;
        Ok(())
    }
    /// Login command to open in Terminal, if the CLI has one.
    fn login_command(&self) -> Option<String> {
        None
    }
    fn can_drive(&self, p: ProviderType) -> bool {
        self.supported_providers().contains(&p)
    }
}

/// What a headless CLI adapter must describe.
pub trait CliSpec: Send + Sync {
    fn bin(&self) -> String;
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()>;
    /// Parse one output line; emit normalized events; capture the final summary.
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent));
}

#[derive(Default)]
pub struct ParseState {
    pub summary: Option<String>,
    pub reported_error: Option<String>,
    /// The CLI reported failure even if it exited 0.
    pub failed: bool,
    pub saw_tokens: bool,
}

/// Shared `run` for CLI adapters: spawn in the workspace at lowered priority,
/// stream and parse output, kill the tree on cancel/timeout.
pub async fn run_cli(spec: &dyn CliSpec, registry: &ProcRegistry, label: &str, req: StepRequest, tx: EventTx) -> Result<StepOutcome> {
    let bin = proc::which(&spec.bin()).ok_or_else(|| anyhow::anyhow!("{} is not installed", spec.bin()))?;
    let mut cmd = proc::command(&bin, req.low_priority);
    cmd.current_dir(&req.workspace);
    spec.build(&req, &mut cmd)?;
    let mut st = ParseState::default();
    let emit = |e: AgentEvent| tx(e);
    // Log the command without the prompt (long) and never any env (keys).
    let args: Vec<String> = cmd
        .as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .map(|a| if a == req.prompt { format!("<prompt {} chars>", a.len()) } else { a })
        .collect();
    log::info!("spawn {} {} (cwd {})", bin.display(), args.join(" "), req.workspace.display());
    let started = std::time::Instant::now();
    let exit =
        proc::run_streaming(cmd, RunOpts { registry, owner: &req.step_id, label, timeout: req.timeout, cancel: &req.cancel }, |s, line| {
            let line = strip_ansi(line);
            if line.trim().is_empty() {
                return;
            }
            spec.parse(s, &line, &mut st, &emit);
        })
        .await?;
    log::info!(
        "{label} exited: {exit:?} after {:.1}s{}",
        started.elapsed().as_secs_f64(),
        st.reported_error.as_deref().map(|e| format!(" · error: {e}")).unwrap_or_default()
    );
    Ok(outcome_from(exit, st))
}

pub fn outcome_from(exit: ExitKind, st: ParseState) -> StepOutcome {
    let (result, code) = match exit {
        ExitKind::Exited(0) if !st.failed => (StepResult::Success, Some(0)),
        ExitKind::Exited(c) => (StepResult::Failure, Some(c)),
        ExitKind::Signaled => (StepResult::Failure, None),
        ExitKind::TimedOut => (StepResult::TimedOut, None),
        ExitKind::Cancelled => (StepResult::Cancelled, None),
    };
    StepOutcome { result, exit_code: code, summary: st.summary, error: st.reported_error }
}

/// Default text parser for CLIs without a JSON mode.
pub fn parse_text_line(stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
    let l = line.trim_end();
    if stream == Stream::Stderr && looks_like_error(l) {
        st.reported_error = Some(l.to_string());
        emit(AgentEvent::Error { text: l.to_string() });
        return;
    }
    emit(AgentEvent::Stdout { text: l.to_string() });
    if !l.trim().is_empty() {
        st.summary = Some(l.trim().to_string());
    }
}

pub fn looks_like_error(l: &str) -> bool {
    let low = l.to_ascii_lowercase();
    low.starts_with("error") || low.contains(" error:") || low.starts_with("fatal")
}

/// Removes ANSI escape sequences (colors, cursor moves) from CLI output.
pub fn strip_ansi(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('\u{1b}') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() || n == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Runs `<bin> --version` (or given args) and pulls out the first version-looking token.
pub async fn detect_bin(bin: &str, args: &[&str]) -> DetectResult {
    let Some(path) = proc::which(bin) else { return DetectResult::default() };
    let p = path.to_string_lossy().to_string();
    let version = match proc::output(&p, args, None, Duration::from_secs(8)).await {
        Ok((_, out, err)) => extract_version(if out.is_empty() { &err } else { &out }),
        Err(_) => None,
    };
    DetectResult { installed: true, version, path: Some(p) }
}

pub fn extract_version(s: &str) -> Option<String> {
    s.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .map(|t| t.trim_start_matches('v'))
        .find(|t| {
            let mut parts = t.split('.');
            matches!((parts.next(), parts.next()), (Some(a), Some(b)) if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().next().is_some_and(|c| c.is_ascii_digit()))
        })
        .map(String::from)
}

/// Short human detail for a tool call's JSON input.
pub fn tool_detail(input: &serde_json::Value) -> String {
    for k in ["file_path", "filePath", "path", "command", "cmd", "pattern", "url", "description"] {
        if let Some(v) = input.get(k) {
            if let Some(s) = v.as_str() {
                return truncate(s, 160);
            }
            if let Some(arr) = v.as_array() {
                let joined: Vec<&str> = arr.iter().filter_map(|x| x.as_str()).collect();
                return truncate(&joined.join(" "), 160);
            }
        }
    }
    String::new()
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

pub fn is_edit_tool(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "edit"
            | "write"
            | "multiedit"
            | "notebookedit"
            | "patch"
            | "apply_patch"
            | "str_replace"
            | "create"
            | "writetoolcall"
            | "edittoolcall"
            | "write_file"
            | "edit_file"
    )
}

/// All adapters the app knows. Order = display order.
pub fn registry(ctx: &AdapterCtx) -> Vec<Arc<dyn AgentAdapter>> {
    let mut v: Vec<Arc<dyn AgentAdapter>> = vec![
        Arc::new(cursor::Cursor::new(ctx.clone())),
        Arc::new(claude::Claude::new(ctx.clone())),
        Arc::new(codex::Codex::new(ctx.clone())),
        Arc::new(copilot::Copilot::new(ctx.clone())),
        Arc::new(opencode::OpenCode::new(ctx.clone())),
        Arc::new(aider::Aider::new(ctx.clone())),
        Arc::new(goose::Goose::new(ctx.clone())),
    ];
    if ctx.demo_agents {
        v.push(Arc::new(fake::Fake::paid(ctx.clone())));
        v.push(Arc::new(fake::Fake::open_source(ctx.clone())));
    }
    v
}

#[derive(Clone)]
pub struct AdapterCtx {
    pub registry: ProcRegistry,
    pub data_dir: PathBuf,
    /// Registers the scripted fake agents (tests, demo mode).
    pub demo_agents: bool,
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use parking_lot::Mutex;

    /// Feeds a fixture through a spec's parser and collects events.
    pub fn parse_fixture(spec: &dyn CliSpec, fixture: &str) -> (Vec<AgentEvent>, ParseState) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let emit = move |e: AgentEvent| ev.lock().push(e);
        let mut st = ParseState::default();
        for line in fixture.lines() {
            if line.trim().is_empty() {
                continue;
            }
            spec.parse(Stream::Stdout, line, &mut st, &emit);
        }
        let v = events.lock().clone();
        (v, st)
    }

    pub fn fixture(adapter: &str, name: &str) -> String {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures").join(adapter).join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("missing fixture {}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m done"), "ok done");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn extracts_versions() {
        assert_eq!(extract_version("2.1.278 (Claude Code)").as_deref(), Some("2.1.278"));
        assert_eq!(extract_version("codex-cli 0.46.0").as_deref(), Some("0.46.0"));
        assert_eq!(extract_version("opencode v1.18.34").as_deref(), Some("1.18.34"));
        assert_eq!(extract_version("no version here"), None);
    }
}

/// Wires a [`CliSpec`] adapter (with a `ctx: AdapterCtx` field) to the
/// trait's default `run` and `cancel`.
#[macro_export]
macro_rules! impl_cli_run {
    () => {
        fn cli_spec(&self) -> Option<&dyn CliSpec> {
            Some(self)
        }
        fn procs(&self) -> &ProcRegistry {
            &self.ctx.registry
        }
    };
}
