//! Types shared across the core and serialized to the UI (snake_case JSON).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

macro_rules! str_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $(#[serde(rename = $s)] $variant),+ }
        impl $name {
            pub fn as_str(&self) -> &'static str { match self { $(Self::$variant => $s),+ } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some(Self::$variant),)+ _ => None } }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.as_str()) }
        }
    };
}

str_enum!(AgentFamily { Paid => "paid", OpenSource => "open_source" });
str_enum!(Tier { Local => "local", CheapCloud => "cheap_cloud", Premium => "premium" });
str_enum!(StepClass { High => "high", Low => "low", Trivial => "trivial" });
str_enum!(StepKind { Plan => "plan", Execute => "execute", Review => "review" });
str_enum!(ProviderType {
    Ollama => "ollama",
    OpenaiCompatible => "openai_compatible",
    Openrouter => "openrouter",
    Anthropic => "anthropic",
    Openai => "openai",
    Gemini => "gemini",
    Lmstudio => "lmstudio",
    Llamacpp => "llamacpp",
    Mlx => "mlx",
    Fake => "fake",
});
str_enum!(RunStatus {
    Pending => "pending",
    Planning => "planning",
    Running => "running",
    Paused => "paused",
    Reviewing => "reviewing",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
});
str_enum!(StepStatus {
    Pending => "pending",
    Running => "running",
    Verifying => "verifying",
    AwaitingApproval => "awaiting_approval",
    Passed => "passed",
    Failed => "failed",
    Accepted => "accepted",
    Skipped => "skipped",
    Cancelled => "cancelled",
    RolledBack => "rolled_back",
});

impl ProviderType {
    /// Runs on this Mac (no data leaves the machine).
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Ollama | Self::Lmstudio | Self::Llamacpp | Self::Mlx)
    }
    /// Speaks the OpenAI chat-completions API at `base_url`.
    pub fn openai_compatible(&self) -> bool {
        !matches!(self, Self::Anthropic)
    }
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// A concrete model on a concrete backend. `None` in a request means "use the
/// CLI's own default model" (paid agents).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelRef {
    pub provider_id: String,
    pub provider_type: ProviderType,
    /// Model name as the provider knows it, e.g. `qwen2.5-coder:7b`.
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Keychain account holding the API key. Never the key itself.
    #[serde(default)]
    pub key_ref: Option<String>,
    pub tier: Tier,
    #[serde(default)]
    pub mem_needed_gb: Option<f64>,
    #[serde(default)]
    pub ctx_len: Option<u32>,
    #[serde(default)]
    pub price_in_per_m: Option<f64>,
    #[serde(default)]
    pub price_out_per_m: Option<f64>,
}

impl ModelRef {
    pub fn label(&self) -> String {
        self.display_name.clone().unwrap_or_else(|| self.name.clone())
    }
}

/// One entry of the executor pool: an open-source agent driving a model.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExecutorEntry {
    pub agent_id: String,
    pub model: ModelRef,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

// How a mode executes each step class.
str_enum!(Exec { Paid => "paid", Cheap => "cheap" });
str_enum!(Escalation { Auto => "auto", Approval => "approval", Never => "never" });

/// A routing mode. The three built-ins can be renamed; users may duplicate
/// one and tweak its table (docs/SPEC.md → Router and modes).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModeDef {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub builtin: bool,
    pub description: String,
    pub high: Exec,
    pub low: Exec,
    pub trivial: Exec,
    /// Class used when the classifier can't decide.
    pub unclear_as: StepClass,
    pub review: bool,
    pub escalation: Escalation,
    /// Failed attempts on one executor before moving up the ladder.
    pub attempts_per_executor: u32,
    pub short_plan: bool,
}

impl ModeDef {
    pub fn exec_for(&self, class: StepClass) -> Exec {
        match class {
            StepClass::High => self.high,
            StepClass::Low => self.low,
            StepClass::Trivial => self.trivial,
        }
    }

    pub fn builtins() -> Vec<ModeDef> {
        vec![
            ModeDef {
                id: "cost".into(),
                display_name: "Cost".into(),
                builtin: true,
                description: "Cheap executors for every step. Paid agent plans, and only steps in when you approve.".into(),
                high: Exec::Cheap,
                low: Exec::Cheap,
                trivial: Exec::Cheap,
                unclear_as: StepClass::Low,
                review: false,
                escalation: Escalation::Approval,
                attempts_per_executor: 2,
                short_plan: true,
            },
            ModeDef {
                id: "balanced".into(),
                display_name: "Balanced".into(),
                builtin: true,
                description: "Paid agent for hard steps, cheap executors for routine ones, one paid review at the end.".into(),
                high: Exec::Paid,
                low: Exec::Cheap,
                trivial: Exec::Cheap,
                unclear_as: StepClass::High,
                review: true,
                escalation: Escalation::Auto,
                attempts_per_executor: 2,
                short_plan: false,
            },
            ModeDef {
                id: "intelligent".into(),
                display_name: "Intelligent".into(),
                builtin: true,
                description: "Paid agent for everything except trivial chores like formatting and commit messages.".into(),
                high: Exec::Paid,
                low: Exec::Paid,
                trivial: Exec::Cheap,
                unclear_as: StepClass::High,
                review: true,
                escalation: Escalation::Auto,
                attempts_per_executor: 2,
                short_plan: false,
            },
        ]
    }
}

/// Price the user sets per paid agent (estimates only).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct AgentPrice {
    #[serde(default)]
    pub in_per_m: Option<f64>,
    #[serde(default)]
    pub out_per_m: Option<f64>,
    /// Effective cost per request for subscription tools (e.g. Cursor).
    #[serde(default)]
    pub per_request: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct RunBudget {
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub max_steps: Option<u32>,
    #[serde(default)]
    pub max_paid_tokens: Option<u64>,
}

pub type Prices = HashMap<String, AgentPrice>;
