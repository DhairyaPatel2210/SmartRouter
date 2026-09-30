//! Codex CLI: `codex exec --json --full-auto --skip-git-repo-check`.
//! With `--oss` it drives a local Ollama model, so it can also act as an
//! open-source-style executor. Schema per Codex docs (see docs/DECISIONS.md).

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Codex {
    ctx: AdapterCtx,
}

impl Codex {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

impl CliSpec for Codex {
    fn bin(&self) -> String {
        "codex".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["exec", "--json", "--full-auto", "--skip-git-repo-check", "--cd"]).arg(&req.workspace);
        if let Some(m) = &req.model {
            if m.provider_type == ProviderType::Ollama {
                cmd.args(["--oss", "-m", &m.name]);
            } else if m.tier == Tier::Premium {
                cmd.args(["-m", &m.name]);
            }
        }
        if let Some(k) = &req.api_key {
            cmd.env("OPENAI_API_KEY", k);
        }
        cmd.arg(&req.prompt);
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return parse_text_line(stream, line, st, emit);
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("item.started") | Some("item.completed") => {
                let completed = v["type"] == "item.completed";
                let item = &v["item"];
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("agent_message") if completed => {
                        let t = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        for l in t.lines().filter(|l| !l.trim().is_empty()) {
                            emit(AgentEvent::Stdout { text: l.to_string() });
                        }
                        st.summary = Some(t.trim().to_string());
                    }
                    Some("command_execution") if !completed => {
                        let c = item.get("command").and_then(|c| c.as_str()).unwrap_or("");
                        emit(AgentEvent::ToolCall { name: "shell".into(), detail: truncate(c, 160) });
                    }
                    Some("file_change") if completed => {
                        for ch in item.get("changes").and_then(|c| c.as_array()).into_iter().flatten() {
                            if let Some(p) = ch.get("path").and_then(|p| p.as_str()) {
                                emit(AgentEvent::FileEdit { path: p.to_string() });
                                emit(AgentEvent::ToolCall { name: "edit".into(), detail: p.to_string() });
                            }
                        }
                    }
                    Some("mcp_tool_call") if !completed => {
                        let t = item.get("tool").and_then(|t| t.as_str()).unwrap_or("mcp");
                        emit(AgentEvent::ToolCall { name: t.into(), detail: String::new() });
                    }
                    Some("error") => {
                        let m = item.get("message").and_then(|m| m.as_str()).unwrap_or("error");
                        emit(AgentEvent::Error { text: m.into() });
                    }
                    _ => {}
                }
            }
            Some("turn.completed") => {
                if let Some(u) = v.get("usage") {
                    let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    st.saw_tokens = true;
                    emit(AgentEvent::Tokens { input: n("input_tokens") + n("cached_input_tokens"), output: n("output_tokens") });
                }
            }
            Some("turn.failed") | Some("error") => {
                let m = v
                    .pointer("/error/message")
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Codex reported an error")
                    .to_string();
                st.failed = true;
                st.reported_error = Some(m.clone());
                emit(AgentEvent::Error { text: m });
            }
            _ => {}
        }
    }
}

#[async_trait]
impl AgentAdapter for Codex {
    fn id(&self) -> &str {
        "codex"
    }
    fn display_name(&self) -> &str {
        "Codex CLI"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::Paid
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[ProviderType::Openai, ProviderType::Ollama]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("codex", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        match proc::output("codex", &["login", "status"], None, Duration::from_secs(8)).await {
            Ok((0, _, _)) => AuthStatus::Ok,
            Ok(_) => AuthStatus::Missing { hint: "Run `codex login`".into() },
            Err(_) => AuthStatus::Unknown,
        }
    }
    fn login_command(&self) -> Option<String> {
        Some("codex login".into())
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    #[test]
    fn parses_json_fixture() {
        let c = Codex::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("codex", "exec.jsonl"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { path } if path == "src/greeting.ts")));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Tokens { .. })));
        assert!(!st.failed);
        assert_eq!(st.summary.as_deref(), Some("Added the greeting module."));
    }
}
