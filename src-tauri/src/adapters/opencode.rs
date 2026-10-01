//! OpenCode: `opencode run --format json -m <provider>/<model> --dir <ws>`.
//! Flags verified against `opencode run --help` (1.18.34). The model config
//! is passed inline via `OPENCODE_CONFIG_CONTENT`, with the API key as an
//! `{env:…}` reference, so nothing is written into the user's workspace and
//! no key ever touches disk.

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;
use serde_json::json;

pub struct OpenCode {
    ctx: AdapterCtx,
}

impl OpenCode {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

pub const KEY_ENV: &str = "ORCH_PROVIDER_API_KEY";

/// Provider id used inside the generated OpenCode config.
fn provider_key(m: &ModelRef) -> String {
    let clean: String = m.provider_id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    format!("orch-{clean}")
}

pub fn default_base_url(p: ProviderType) -> &'static str {
    match p {
        ProviderType::Ollama => "http://127.0.0.1:11434",
        ProviderType::Lmstudio => "http://127.0.0.1:1234/v1",
        ProviderType::Llamacpp => "http://127.0.0.1:8080/v1",
        ProviderType::Mlx => "http://127.0.0.1:8080/v1",
        ProviderType::Openrouter => "https://openrouter.ai/api/v1",
        ProviderType::Openai => "https://api.openai.com/v1",
        ProviderType::Anthropic => "https://api.anthropic.com/v1",
        ProviderType::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
        ProviderType::OpenaiCompatible | ProviderType::Fake => "http://127.0.0.1:8000/v1",
    }
}

/// OpenAI-compatible base URL (Ollama's lives under `/v1`).
pub fn openai_base(m: &ModelRef) -> String {
    let base = m.base_url.clone().unwrap_or_else(|| default_base_url(m.provider_type).to_string());
    let base = base.trim_end_matches('/').to_string();
    if m.provider_type == ProviderType::Ollama && !base.ends_with("/v1") {
        format!("{base}/v1")
    } else {
        base
    }
}

/// Returns (inline config JSON, `-m` argument).
pub fn config_for(m: &ModelRef) -> (String, String) {
    let pk = provider_key(m);
    let mut model_entry = json!({ "name": m.label(), "tool_call": true });
    if let Some(ctx) = m.ctx_len {
        model_entry["limit"] = json!({ "context": ctx, "output": 8192.min(ctx / 2) });
    }
    let mut options = json!({ "baseURL": openai_base(m) });
    if m.key_ref.is_some() || !m.provider_type.is_local() {
        options["apiKey"] = json!(format!("{{env:{KEY_ENV}}}"));
    } else {
        options["apiKey"] = json!("local");
    }
    let npm = if m.provider_type == ProviderType::Anthropic { "@ai-sdk/anthropic" } else { "@ai-sdk/openai-compatible" };
    let cfg = json!({
        "$schema": "https://opencode.ai/config.json",
        "autoupdate": false,
        "share": "disabled",
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "webfetch": "allow",
            "external_directory": "deny"
        },
        "provider": {
            pk.clone(): {
                "npm": npm,
                "name": m.provider_id,
                "options": options,
                "models": { m.name.clone(): model_entry }
            }
        }
    });
    (cfg.to_string(), format!("{pk}/{}", m.name))
}

/// `{"name": "write", "arguments": {...}}` printed as text instead of a real tool call.
fn looks_like_text_tool_call(t: &str) -> bool {
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("\"name\":") && (compact.contains("\"arguments\":") || compact.contains("\"parameters\":"))
}

impl CliSpec for OpenCode {
    fn bin(&self) -> String {
        "opencode".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["run", "--format", "json", "--dir"]).arg(&req.workspace);
        if let Some(m) = &req.model {
            let (cfg, model_arg) = config_for(m);
            cmd.env("OPENCODE_CONFIG_CONTENT", cfg).args(["-m", &model_arg]);
        }
        if let Some(a) = &req.library_agent {
            cmd.args(["--agent", a]);
        }
        if let Some(k) = &req.api_key {
            cmd.env(KEY_ENV, k);
        }
        cmd.env("OPENCODE_DISABLE_AUTOUPDATE", "1").env("OPENCODE_DISABLE_SHARE", "1");
        cmd.arg(&req.prompt);
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return parse_text_line(stream, line, st, emit);
        };
        let part = v.get("part").cloned().unwrap_or_default();
        match v.get("type").and_then(|t| t.as_str()) {
            Some("step_start") => st.tool_in_turn = false,
            Some("text") => {
                let t = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if looks_like_text_tool_call(t) {
                    st.text_tool_calls += 1;
                }
                for l in t.lines().filter(|l| !l.trim().is_empty()) {
                    emit(AgentEvent::Stdout { text: l.to_string() });
                }
                if !t.trim().is_empty() {
                    st.summary = Some(t.trim().to_string());
                }
            }
            Some("tool_use") => {
                st.tool_in_turn = true;
                st.text_only_turns = 0;
                let name = part.get("tool").and_then(|t| t.as_str()).unwrap_or("tool").to_string();
                let input = part.pointer("/state/input").cloned().unwrap_or_default();
                let detail = {
                    let d = tool_detail(&input);
                    if d.is_empty() {
                        part.pointer("/state/title").and_then(|t| t.as_str()).unwrap_or("").to_string()
                    } else {
                        d
                    }
                };
                if is_edit_tool(&name) {
                    if let Some(p) = input.get("filePath").or_else(|| input.get("path")).and_then(|p| p.as_str()) {
                        emit(AgentEvent::FileEdit { path: p.to_string() });
                    }
                }
                if part.pointer("/state/status").and_then(|s| s.as_str()) == Some("error") {
                    let err = part.pointer("/state/error").and_then(|e| e.as_str()).unwrap_or("tool failed");
                    emit(AgentEvent::Error { text: format!("{name}: {}", truncate(err, 300)) });
                }
                emit(AgentEvent::ToolCall { name, detail });
            }
            Some("step_finish") => {
                if !st.tool_in_turn {
                    st.text_only_turns += 1;
                }
                // Small local models often print tool calls as text and loop
                // (verified: qwen2.5-coder:3b). Stop after 3 tool-less turns.
                if st.text_only_turns >= 3 && st.abort.is_none() {
                    st.abort = Some(if st.text_tool_calls > 0 {
                        "The model wrote its tool calls as plain text instead of calling tools, so it can't edit files. Use a model with reliable tool calling (a larger local model or a cloud model).".into()
                    } else {
                        "The agent replied 3 times without using any tools, so it was stopped as stuck.".into()
                    });
                }
                if let Some(t) = part.get("tokens") {
                    let n = |p: &str| t.pointer(p).and_then(|x| x.as_u64()).unwrap_or(0);
                    st.saw_tokens = true;
                    emit(AgentEvent::Tokens { input: n("/input") + n("/cache/read"), output: n("/output") + n("/reasoning") });
                }
                if let Some(c) = part.get("cost").and_then(|c| c.as_f64()) {
                    if c > 0.0 {
                        emit(AgentEvent::Cost { usd: c });
                    }
                }
            }
            Some("error") => {
                let msg = v
                    .pointer("/error/data/message")
                    .or_else(|| v.pointer("/error/message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| v.get("error").map(|e| e.to_string()).unwrap_or_default());
                st.failed = true;
                st.reported_error = Some(msg.clone());
                emit(AgentEvent::Error { text: msg });
            }
            _ => {}
        }
    }
}

#[async_trait]
impl AgentAdapter for OpenCode {
    fn id(&self) -> &str {
        "opencode"
    }
    fn display_name(&self) -> &str {
        "OpenCode"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::OpenSource
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[
            ProviderType::Ollama,
            ProviderType::Lmstudio,
            ProviderType::Llamacpp,
            ProviderType::Mlx,
            ProviderType::OpenaiCompatible,
            ProviderType::Openrouter,
            ProviderType::Openai,
            ProviderType::Anthropic,
            ProviderType::Gemini,
            ProviderType::Fake,
        ]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("opencode", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        // Credentials come from the model provider we configure per run.
        AuthStatus::Ok
    }
    async fn configure_model(&self, _ws: &Workspace, model: &ModelRef) -> Result<()> {
        if !self.can_drive(model.provider_type) {
            anyhow::bail!("OpenCode can't drive {} models", model.provider_type);
        }
        Ok(())
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    fn model(p: ProviderType) -> ModelRef {
        ModelRef {
            provider_id: "ollama".into(),
            provider_type: p,
            name: "qwen2.5-coder:7b".into(),
            display_name: None,
            base_url: None,
            key_ref: None,
            tier: Tier::Local,
            mem_needed_gb: None,
            ctx_len: Some(16384),
            price_in_per_m: None,
            price_out_per_m: None,
        }
    }

    #[test]
    fn config_never_contains_key_and_uses_v1() {
        let (cfg, m) = config_for(&model(ProviderType::Ollama));
        assert_eq!(m, "orch-ollama/qwen2.5-coder:7b");
        assert!(cfg.contains("http://127.0.0.1:11434/v1"));
        let mut cloud = model(ProviderType::Openrouter);
        cloud.provider_id = "openrouter".into();
        cloud.key_ref = Some("openrouter".into());
        let (cfg, _) = config_for(&cloud);
        assert!(cfg.contains("{env:ORCH_PROVIDER_API_KEY}"));
        assert!(cfg.contains("external_directory"));
    }

    #[test]
    fn stops_a_model_that_never_calls_tools() {
        let c = OpenCode::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (_, st) = parse_fixture(&c, &fixture("opencode", "text-tool-calls.jsonl"));
        let reason = st.abort.expect("should abort");
        assert!(reason.contains("plain text"), "{reason}");
    }

    #[test]
    fn parses_recorded_fixture() {
        let c = OpenCode::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("opencode", "run.jsonl"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { .. })), "{ev:?}");
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Tokens { .. })));
        assert!(!st.failed);
        assert!(st.summary.is_some());
    }
}
