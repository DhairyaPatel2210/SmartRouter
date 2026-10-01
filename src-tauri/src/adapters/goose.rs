//! Goose: `goose run --no-session -t <prompt>`, provider/model via env
//! (`GOOSE_PROVIDER`, `GOOSE_MODEL`). Plain-text output.

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Goose {
    ctx: AdapterCtx,
}

impl Goose {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

impl CliSpec for Goose {
    fn bin(&self) -> String {
        "goose".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["run", "--no-session", "-t", &req.prompt]);
        if let Some(m) = &req.model {
            let base = super::opencode::openai_base(m);
            match m.provider_type {
                ProviderType::Ollama => {
                    cmd.env("GOOSE_PROVIDER", "ollama").env("OLLAMA_HOST", base.trim_end_matches("/v1"));
                }
                ProviderType::Openrouter => {
                    cmd.env("GOOSE_PROVIDER", "openrouter");
                    if let Some(k) = &req.api_key {
                        cmd.env("OPENROUTER_API_KEY", k);
                    }
                }
                ProviderType::Anthropic => {
                    cmd.env("GOOSE_PROVIDER", "anthropic");
                    if let Some(k) = &req.api_key {
                        cmd.env("ANTHROPIC_API_KEY", k);
                    }
                }
                _ => {
                    // Goose's OpenAI provider accepts a custom host + base path.
                    let (host, path) = split_base(&base);
                    cmd.env("GOOSE_PROVIDER", "openai")
                        .env("OPENAI_HOST", host)
                        .env("OPENAI_BASE_PATH", format!("{path}/chat/completions"));
                    cmd.env("OPENAI_API_KEY", req.api_key.as_deref().unwrap_or("local"));
                }
            }
            cmd.env("GOOSE_MODEL", &m.name);
        }
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let t = line.trim();
        // Goose prints tool calls as "─── text_editor | developer ──────".
        if t.starts_with("───") || t.starts_with("---") {
            let inner = t.trim_matches(|c| c == '─' || c == '-' || c == ' ');
            if let Some((tool, _)) = inner.split_once('|') {
                emit(AgentEvent::ToolCall { name: tool.trim().into(), detail: String::new() });
                return;
            }
        }
        if let Some(p) = t.strip_prefix("path: ") {
            emit(AgentEvent::FileEdit { path: p.trim().to_string() });
            return;
        }
        parse_text_line(stream, line, st, emit)
    }
}

fn split_base(base: &str) -> (String, String) {
    if let Some(i) = base.find("://") {
        if let Some(j) = base[i + 3..].find('/') {
            let cut = i + 3 + j;
            return (base[..cut].to_string(), base[cut + 1..].to_string());
        }
    }
    (base.to_string(), "v1".to_string())
}

#[async_trait]
impl AgentAdapter for Goose {
    fn id(&self) -> &str {
        "goose"
    }
    fn display_name(&self) -> &str {
        "Goose"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::OpenSource
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[
            ProviderType::Ollama,
            ProviderType::OpenaiCompatible,
            ProviderType::Openrouter,
            ProviderType::Openai,
            ProviderType::Anthropic,
            ProviderType::Lmstudio,
            ProviderType::Llamacpp,
            ProviderType::Mlx,
            ProviderType::Fake,
        ]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("goose", &["--version"]).await
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
    fn base_split() {
        assert_eq!(split_base("https://api.deepseek.com/v1"), ("https://api.deepseek.com".into(), "v1".into()));
    }

    #[test]
    fn parses_text_fixture() {
        let c = Goose::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("goose", "output.txt"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolCall { name, .. } if name == "text_editor")));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { .. })));
        assert!(st.summary.is_some());
    }
}
