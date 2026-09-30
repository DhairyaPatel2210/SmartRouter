//! Aider: `aider --message <prompt> --yes-always --no-auto-commits …`.
//! Local models via `ollama_chat/<name>` + `OLLAMA_API_BASE`; OpenAI-compatible
//! via `openai/<name>` + `OPENAI_API_BASE`.

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Aider {
    ctx: AdapterCtx,
}

impl Aider {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

/// Parses "Tokens: 2.1k sent, 312 received." into (in, out).
fn parse_tokens(l: &str) -> Option<(u64, u64)> {
    let rest = l.strip_prefix("Tokens:")?;
    let num = |s: &str| -> Option<u64> {
        let s = s.trim().replace(',', "");
        if let Some(k) = s.strip_suffix('k') {
            k.parse::<f64>().ok().map(|f| (f * 1000.0) as u64)
        } else {
            s.parse::<f64>().ok().map(|f| f as u64)
        }
    };
    let mut sent = None;
    let mut recv = None;
    let words: Vec<&str> = rest.split_whitespace().collect();
    for w in words.windows(2) {
        let v = w[0].trim_end_matches(',');
        if w[1].starts_with("sent") {
            sent = num(v);
        } else if w[1].starts_with("received") {
            recv = num(v);
        }
    }
    Some((sent?, recv?))
}

impl CliSpec for Aider {
    fn bin(&self) -> String {
        "aider".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args([
            "--message", &req.prompt, "--yes-always", "--no-auto-commits", "--no-pretty", "--no-stream",
            "--no-check-update", "--no-show-model-warnings", "--no-analytics", "--no-gitignore",
        ]);
        if req.workspace.join("AGENTS.md").exists() {
            cmd.args(["--read", "AGENTS.md"]);
        }
        if let Some(m) = &req.model {
            let base = super::opencode::openai_base(m);
            let model = match m.provider_type {
                ProviderType::Ollama => {
                    cmd.env("OLLAMA_API_BASE", base.trim_end_matches("/v1"));
                    format!("ollama_chat/{}", m.name)
                }
                ProviderType::Openrouter => {
                    if let Some(k) = &req.api_key {
                        cmd.env("OPENROUTER_API_KEY", k);
                    }
                    format!("openrouter/{}", m.name)
                }
                ProviderType::Anthropic => {
                    if let Some(k) = &req.api_key {
                        cmd.env("ANTHROPIC_API_KEY", k);
                    }
                    format!("anthropic/{}", m.name)
                }
                _ => {
                    cmd.env("OPENAI_API_BASE", base);
                    cmd.env("OPENAI_API_KEY", req.api_key.as_deref().unwrap_or("local"));
                    format!("openai/{}", m.name)
                }
            };
            cmd.args(["--model", &model]);
        }
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let t = line.trim();
        if let Some(p) = t.strip_prefix("Applied edit to ") {
            emit(AgentEvent::FileEdit { path: p.trim().to_string() });
            emit(AgentEvent::ToolCall { name: "edit".into(), detail: p.trim().to_string() });
            return;
        }
        if let Some((i, o)) = parse_tokens(t) {
            st.saw_tokens = true;
            emit(AgentEvent::Tokens { input: i, output: o });
            return;
        }
        parse_text_line(stream, line, st, emit)
    }
}

#[async_trait]
impl AgentAdapter for Aider {
    fn id(&self) -> &str {
        "aider"
    }
    fn display_name(&self) -> &str {
        "Aider"
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
        detect_bin("aider", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        AuthStatus::Ok
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    #[test]
    fn token_line() {
        assert_eq!(parse_tokens("Tokens: 2.1k sent, 312 received. Cost: $0.00 message"), Some((2100, 312)));
        assert_eq!(parse_tokens("Tokens: 900 sent, 45 received."), Some((900, 45)));
    }

    #[test]
    fn parses_text_fixture() {
        let c = Aider::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, _) = parse_fixture(&c, &fixture("aider", "output.txt"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { path } if path == "src/greeting.py")));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Tokens { .. })));
    }
}
