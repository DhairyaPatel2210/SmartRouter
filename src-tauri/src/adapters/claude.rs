//! Claude Code: `claude -p --output-format stream-json --verbose`.
//! Flags verified against `claude --help` (2.1.278), see docs/DECISIONS.md.

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Claude {
    ctx: AdapterCtx,
}

impl Claude {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

impl CliSpec for Claude {
    fn bin(&self) -> String {
        "claude".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits"]);
        // Edits are auto-accepted; tools are limited to the workspace (Claude
        // Code confines file access to the cwd unless --add-dir is given).
        cmd.args(["--allowedTools", "Read Edit Write MultiEdit Glob Grep Bash TodoWrite"]);
        if let Some(m) = &req.model {
            if m.provider_type == ProviderType::Anthropic || m.tier == Tier::Premium {
                cmd.args(["--model", &m.name]);
            }
        }
        if let Some(a) = &req.library_agent {
            cmd.args(["--agent", a]);
        }
        if let Some(k) = &req.api_key {
            cmd.env("ANTHROPIC_API_KEY", k);
        }
        cmd.arg(&req.prompt);
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return parse_text_line(stream, line, st, emit);
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("system") => {
                if v.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                    let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("default model");
                    emit(AgentEvent::Stdout { text: format!("Claude Code started ({model})") });
                }
            }
            Some("assistant") => {
                let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) else { return };
                for c in content {
                    match c.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            let t = c.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            for l in t.lines().filter(|l| !l.trim().is_empty()) {
                                emit(AgentEvent::Stdout { text: l.to_string() });
                            }
                        }
                        Some("tool_use") => {
                            let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
                            let input = c.get("input").cloned().unwrap_or_default();
                            let detail = tool_detail(&input);
                            if is_edit_tool(&name) {
                                if let Some(p) = input.get("file_path").or_else(|| input.get("notebook_path")).and_then(|p| p.as_str()) {
                                    emit(AgentEvent::FileEdit { path: p.to_string() });
                                }
                            }
                            emit(AgentEvent::ToolCall { name, detail });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                    for c in content {
                        if c.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                            let text = c.get("content").map(|x| match x.as_str() {
                                Some(s) => s.to_string(),
                                None => x.to_string(),
                            });
                            emit(AgentEvent::Error { text: truncate(&text.unwrap_or_default(), 300) });
                        }
                    }
                }
            }
            Some("result") => {
                if let Some(u) = v.get("usage") {
                    let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    st.saw_tokens = true;
                    emit(AgentEvent::Tokens {
                        input: n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"),
                        output: n("output_tokens"),
                    });
                }
                if let Some(c) = v.get("total_cost_usd").and_then(|c| c.as_f64()) {
                    emit(AgentEvent::Cost { usd: c });
                }
                let result = v.get("result").and_then(|r| r.as_str()).map(String::from);
                if v.get("is_error").and_then(|e| e.as_bool()) == Some(true)
                    || v.get("subtype").and_then(|s| s.as_str()).is_some_and(|s| s.starts_with("error"))
                {
                    st.failed = true;
                    st.reported_error = result.clone().or(Some("Claude Code reported an error".into()));
                }
                st.summary = result;
            }
            _ => {}
        }
    }
}

#[async_trait]
impl AgentAdapter for Claude {
    fn id(&self) -> &str {
        "claude"
    }
    fn display_name(&self) -> &str {
        "Claude Code"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::Paid
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[ProviderType::Anthropic]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("claude", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        match proc::output("claude", &["auth", "status", "--json"], None, Duration::from_secs(8)).await {
            Ok((0, out, _)) => {
                let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
                let logged = v.get("loggedIn").or_else(|| v.get("logged_in")).and_then(|b| b.as_bool());
                match logged {
                    Some(true) => AuthStatus::Ok,
                    Some(false) => AuthStatus::Missing { hint: "Run `claude auth login`".into() },
                    None => AuthStatus::Ok,
                }
            }
            Ok(_) => AuthStatus::Missing { hint: "Run `claude auth login`".into() },
            Err(_) => AuthStatus::Unknown,
        }
    }
    fn login_command(&self) -> Option<String> {
        Some("claude auth login".into())
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    #[test]
    fn parses_stream_json_fixture() {
        let c = Claude::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("claude", "stream.jsonl"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { path } if path.ends_with("greeting.test.ts"))));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolCall { name, .. } if name == "Bash")));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Tokens { input, output } if *input > 0 && *output > 0)));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Cost { .. })));
        assert!(!st.failed);
        assert!(st.summary.unwrap().contains("Added"));
    }
}
