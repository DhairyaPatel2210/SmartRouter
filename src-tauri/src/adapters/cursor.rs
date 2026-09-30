//! Cursor Agent CLI: `cursor-agent -p --output-format stream-json --force`.
//! Not installed on the build machine; schema follows Cursor's CLI docs
//! (see docs/DECISIONS.md) and the fixture under tests/fixtures/cursor.

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Cursor {
    ctx: AdapterCtx,
}

impl Cursor {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

/// `writeToolCall` → `write`, `shellToolCall` → `shell`.
fn tool_name(key: &str) -> String {
    key.strip_suffix("ToolCall").unwrap_or(key).to_string()
}

impl CliSpec for Cursor {
    fn bin(&self) -> String {
        "cursor-agent".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["-p", "--output-format", "stream-json", "--force"]);
        if let Some(m) = &req.model {
            if m.tier == Tier::Premium {
                cmd.args(["--model", &m.name]);
            }
        }
        if let Some(k) = &req.api_key {
            cmd.env("CURSOR_API_KEY", k);
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
                let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("default model");
                emit(AgentEvent::Stdout { text: format!("Cursor Agent started ({model})") });
            }
            Some("assistant") => {
                if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                    for c in content {
                        if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                            for l in t.lines().filter(|l| !l.trim().is_empty()) {
                                emit(AgentEvent::Stdout { text: l.to_string() });
                            }
                        }
                    }
                }
            }
            Some("tool_call") => {
                if v.get("subtype").and_then(|s| s.as_str()) != Some("started") {
                    return;
                }
                let Some(obj) = v.get("tool_call").and_then(|t| t.as_object()) else { return };
                for (key, val) in obj {
                    let name = tool_name(key);
                    let args = val.get("args").cloned().unwrap_or_default();
                    let detail = tool_detail(&args);
                    if matches!(name.as_str(), "write" | "edit" | "delete") || is_edit_tool(key) {
                        if let Some(p) = args.get("path").and_then(|p| p.as_str()) {
                            emit(AgentEvent::FileEdit { path: p.to_string() });
                        }
                    }
                    emit(AgentEvent::ToolCall { name, detail });
                }
            }
            Some("result") => {
                if let Some(u) = v.get("usage") {
                    let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    st.saw_tokens = true;
                    emit(AgentEvent::Tokens { input: n("input_tokens"), output: n("output_tokens") });
                }
                let result = v.get("result").and_then(|r| r.as_str()).map(String::from);
                if v.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                    st.failed = true;
                    st.reported_error = result.clone();
                }
                st.summary = result;
            }
            _ => {}
        }
    }
}

#[async_trait]
impl AgentAdapter for Cursor {
    fn id(&self) -> &str {
        "cursor"
    }
    fn display_name(&self) -> &str {
        "Cursor Agent"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::Paid
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("cursor-agent", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        match proc::output("cursor-agent", &["status"], None, Duration::from_secs(8)).await {
            Ok((0, out, _)) if !out.to_ascii_lowercase().contains("not logged in") => AuthStatus::Ok,
            Ok(_) => AuthStatus::Missing { hint: "Run `cursor-agent login`".into() },
            Err(_) => AuthStatus::Unknown,
        }
    }
    fn login_command(&self) -> Option<String> {
        Some("cursor-agent login".into())
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    #[test]
    fn parses_stream_json_fixture() {
        let c = Cursor::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("cursor", "stream.jsonl"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { path } if path.ends_with("plan.md"))));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolCall { name, .. } if name == "shell")));
        assert!(!st.failed);
        assert!(st.summary.is_some());
    }
}
