//! Fake adapter: drives the scripted fake CLI (`--fake-agent`). Registered
//! as one paid and one open-source agent so full runs can be tested (and
//! demoed) without any real agent or model.

use super::*;
use crate::proc::Stream;

pub struct Fake {
    id: &'static str,
    name: &'static str,
    family: AgentFamily,
    ctx: AdapterCtx,
}

const ALL_PROVIDERS: &[ProviderType] = &[
    ProviderType::Ollama,
    ProviderType::OpenaiCompatible,
    ProviderType::Openrouter,
    ProviderType::Anthropic,
    ProviderType::Openai,
    ProviderType::Gemini,
    ProviderType::Lmstudio,
    ProviderType::Llamacpp,
    ProviderType::Mlx,
    ProviderType::Fake,
];

impl Fake {
    pub fn paid(ctx: AdapterCtx) -> Self {
        Self { id: "fake-paid", name: "Demo paid agent", family: AgentFamily::Paid, ctx }
    }
    pub fn open_source(ctx: AdapterCtx) -> Self {
        Self { id: "fake-oss", name: "Demo open-source agent", family: AgentFamily::OpenSource, ctx }
    }
}

/// Binary that implements `--fake-agent`: `ORCH_FAKE_AGENT_BIN` (tests) or this app.
pub fn fake_bin() -> Option<String> {
    std::env::var("ORCH_FAKE_AGENT_BIN").ok().or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()))
}

pub struct FakeSpec;

impl CliSpec for FakeSpec {
    fn bin(&self) -> String {
        fake_bin().unwrap_or_default()
    }
    fn build(&self, req: &StepRequest, cmd: &mut Command) -> Result<()> {
        cmd.arg("--fake-agent");
        if let Some(m) = &req.model {
            cmd.env("FAKE_AGENT_MODEL", m.label());
        }
        cmd.arg(&req.prompt);
        Ok(())
    }
    fn parse(&self, _s: Stream, line: &str, st: &mut ParseState, emit: &dyn Fn(AgentEvent)) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            emit(AgentEvent::Stdout { text: line.to_string() });
            return;
        };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        match v.get("t").and_then(|t| t.as_str()) {
            Some("text") => emit(AgentEvent::Stdout { text: s("v") }),
            Some("tool") => {
                let name = s("name");
                let path = s("path");
                emit(AgentEvent::ToolCall { name: name.clone(), detail: path.clone() });
                if name == "write" {
                    emit(AgentEvent::FileEdit { path });
                }
            }
            Some("tokens") => {
                st.saw_tokens = true;
                emit(AgentEvent::Tokens {
                    input: v.get("in").and_then(|x| x.as_u64()).unwrap_or(0),
                    output: v.get("out").and_then(|x| x.as_u64()).unwrap_or(0),
                })
            }
            Some("error") => {
                st.reported_error = Some(s("v"));
                emit(AgentEvent::Error { text: s("v") })
            }
            Some("result") => {
                st.summary = Some(s("summary"));
                if v.get("ok").and_then(|x| x.as_bool()) == Some(false) {
                    st.failed = true;
                }
            }
            _ => emit(AgentEvent::Stdout { text: line.to_string() }),
        }
    }
}

#[async_trait]
impl AgentAdapter for Fake {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.name
    }
    fn family(&self) -> AgentFamily {
        self.family
    }
    fn supported_providers(&self) -> &[ProviderType] {
        if self.family == AgentFamily::OpenSource {
            ALL_PROVIDERS
        } else {
            &[]
        }
    }
    async fn detect(&self) -> DetectResult {
        DetectResult { installed: fake_bin().is_some(), version: Some("1.0.0".into()), path: fake_bin() }
    }
    async fn check_auth(&self) -> AuthStatus {
        AuthStatus::Ok
    }
    async fn run(&self, req: StepRequest, tx: EventTx) -> Result<StepOutcome> {
        let bin = fake_bin().ok_or_else(|| anyhow::anyhow!("fake agent binary not found"))?;
        let mut cmd = proc::command(&bin, req.low_priority);
        cmd.current_dir(&req.workspace);
        FakeSpec.build(&req, &mut cmd)?;
        let mut st = ParseState::default();
        let emit = |e: AgentEvent| tx(e);
        let exit = proc::run_streaming(
            cmd,
            RunOpts { registry: &self.ctx.registry, owner: &req.step_id, label: self.name, timeout: req.timeout, cancel: &req.cancel },
            |s, l| FakeSpec.parse(s, l, &mut st, &emit),
        )
        .await?;
        Ok(outcome_from(exit, st))
    }
    fn procs(&self) -> &ProcRegistry {
        &self.ctx.registry
    }
}
