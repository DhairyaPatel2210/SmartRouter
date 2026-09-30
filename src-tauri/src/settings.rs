//! Global settings (one JSON document in the `settings` table) and
//! per-workspace overrides (the `workspaces.settings_json` column).

use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub onboarded: bool,
    pub default_mode: String,
    pub modes: Vec<ModeDef>,
    /// Paid agent used for planning, review and high steps.
    pub planner_agent: Option<String>,
    /// Budget planning: a cheap-cloud model plans and reviews instead of a paid agent.
    pub budget_planning: bool,
    pub budget_planner: Option<ExecutorEntry>,
    /// Ordered (open-source agent, model) pairs; first one the governor allows wins.
    pub executor_pool: Vec<ExecutorEntry>,
    pub approve_before_paid: bool,
    pub approve_before_cloud: bool,
    pub run_budget: RunBudget,
    pub agent_prices: Prices,
    /// Providers whose one-time "code leaves your Mac" notice was acknowledged.
    pub cloud_notice_ack: Vec<String>,

    // Resource governor
    /// Share of unified memory local models may use. `None` = auto (60% ≤16 GB, else 70%).
    pub memory_budget_pct: Option<u8>,
    pub idle_unload_minutes: u32,
    pub prefer_cloud_on_battery: bool,
    pub prefer_cloud_when_hot: bool,
    pub lower_priority: bool,
    pub global_concurrency: u32,
    pub step_timeout_minutes: u32,

    // Models
    /// Where local models live. `None` = runtime default (e.g. ~/.ollama/models).
    pub model_folder: Option<String>,
    /// Start Ollama on demand when it isn't running (and stop it on quit).
    pub manage_ollama: bool,
    pub ollama_always_on: bool,

    // UI
    pub menu_bar: bool,
    pub theme: String,
    pub log_ring_lines: usize,
    pub notifications: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut prices = HashMap::new();
        // Placeholder estimates; the user edits these in Settings → Prices.
        prices.insert("claude".into(), AgentPrice { in_per_m: Some(3.0), out_per_m: Some(15.0), per_request: None });
        prices.insert("codex".into(), AgentPrice { in_per_m: Some(1.25), out_per_m: Some(10.0), per_request: None });
        prices.insert("cursor".into(), AgentPrice { in_per_m: None, out_per_m: None, per_request: Some(0.04) });
        prices.insert("copilot".into(), AgentPrice { in_per_m: None, out_per_m: None, per_request: Some(0.04) });
        prices.insert("fake-paid".into(), AgentPrice { in_per_m: Some(3.0), out_per_m: Some(15.0), per_request: None });
        Self {
            onboarded: false,
            default_mode: "balanced".into(),
            modes: ModeDef::builtins(),
            planner_agent: None,
            budget_planning: false,
            budget_planner: None,
            executor_pool: vec![],
            approve_before_paid: false,
            approve_before_cloud: false,
            run_budget: RunBudget::default(),
            agent_prices: prices,
            cloud_notice_ack: vec![],
            memory_budget_pct: None,
            idle_unload_minutes: 5,
            prefer_cloud_on_battery: true,
            prefer_cloud_when_hot: true,
            lower_priority: true,
            global_concurrency: 2,
            step_timeout_minutes: 15,
            model_folder: None,
            manage_ollama: true,
            ollama_always_on: false,
            menu_bar: true,
            theme: "system".into(),
            log_ring_lines: 2000,
            notifications: true,
        }
    }
}

impl Settings {
    pub fn mode(&self, id: &str) -> ModeDef {
        self.modes
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .or_else(|| ModeDef::builtins().into_iter().find(|m| m.id == id))
            .unwrap_or_else(|| ModeDef::builtins().remove(1))
    }

    /// Makes sure the three built-in modes exist (a user may have renamed them).
    pub fn normalize(&mut self) {
        for b in ModeDef::builtins() {
            if !self.modes.iter().any(|m| m.id == b.id) {
                self.modes.push(b);
            }
        }
        if self.log_ring_lines == 0 {
            self.log_ring_lines = 2000;
        }
        if self.global_concurrency == 0 {
            self.global_concurrency = 1;
        }
    }
}

/// What to do with uncommitted changes before a run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DirtyStrategy {
    #[default]
    Stash,
    Commit,
    RunOnTop,
}

/// Per-workspace overrides; `None` falls back to the global setting.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WorkspaceSettings {
    pub check_commands: Option<Vec<String>>,
    pub run_budget: Option<RunBudget>,
    pub approve_before_paid: Option<bool>,
    pub approve_before_cloud: Option<bool>,
    pub dirty_strategy: Option<DirtyStrategy>,
    /// Commit generated CLI files instead of git-ignoring them.
    pub commit_generated: bool,
    pub import_offered: bool,
}
