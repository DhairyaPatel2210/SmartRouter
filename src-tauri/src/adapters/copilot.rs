//! GitHub Copilot CLI: `copilot -p <prompt> --allow-all-tools` (plain text).

use super::*;
use crate::impl_cli_run;
use crate::proc::Stream;

pub struct Copilot {
    ctx: AdapterCtx,
}

impl Copilot {
    pub fn new(ctx: AdapterCtx) -> Self {
        Self { ctx }
    }
}

impl CliSpec for Copilot {
    fn bin(&self) -> String {
        "copilot".into()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.args(["-p", &req.prompt, "--allow-all-tools", "--no-color"]);
        if let Some(m) = &req.model {
            if m.tier == Tier::Premium {
                cmd.args(["--model", &m.name]);
            }
        }
        if let Some(a) = &req.library_agent {
            cmd.args(["--agent", a]);
        }
        Ok(())
    }
    fn parse(&self, stream: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let t = line.trim();
        // Copilot prints tool activity as "✓ Edit src/x.ts" / "● Run npm test".
        let body = t.trim_start_matches(['✓', '✗', '●', '○', '$', ' ']);
        if body.len() != t.len() {
            let (name, rest) = body.split_once(' ').unwrap_or((body, ""));
            let lname = name.to_ascii_lowercase();
            if matches!(lname.as_str(), "edit" | "create" | "write") && !rest.is_empty() {
                emit(AgentEvent::FileEdit { path: rest.split_whitespace().next().unwrap_or(rest).to_string() });
            }
            emit(AgentEvent::ToolCall { name: lname, detail: truncate(rest, 160) });
            if t.starts_with('✗') {
                emit(AgentEvent::Error { text: body.to_string() });
            }
            return;
        }
        parse_text_line(stream, line, st, emit)
    }
}

#[async_trait]
impl AgentAdapter for Copilot {
    fn id(&self) -> &str {
        "copilot"
    }
    fn display_name(&self) -> &str {
        "GitHub Copilot CLI"
    }
    fn family(&self) -> AgentFamily {
        AgentFamily::Paid
    }
    fn supported_providers(&self) -> &[ProviderType] {
        &[]
    }
    async fn detect(&self) -> DetectResult {
        detect_bin("copilot", &["--version"]).await
    }
    async fn check_auth(&self) -> AuthStatus {
        if std::env::var("GH_TOKEN").is_ok() || std::env::var("GITHUB_TOKEN").is_ok() {
            return AuthStatus::Ok;
        }
        let cfg = dirs::home_dir().map(|h| h.join(".copilot/config.json"));
        match cfg {
            Some(p) if p.exists() => AuthStatus::Ok,
            _ => AuthStatus::Missing { hint: "Run `copilot` and use /login".into() },
        }
    }
    fn login_command(&self) -> Option<String> {
        Some("copilot".into())
    }
    impl_cli_run!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::*;

    #[test]
    fn parses_text_fixture() {
        let c = Copilot::new(AdapterCtx { registry: Default::default(), data_dir: "/tmp".into(), demo_agents: false });
        let (ev, st) = parse_fixture(&c, &fixture("copilot", "output.txt"));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::FileEdit { path } if path == "src/greeting.ts")));
        assert!(st.summary.is_some());
    }
}
