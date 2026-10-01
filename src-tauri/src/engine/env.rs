//! Builds the router's inputs (which executors are usable right now, and
//! why not) and the pre-flight summary shown before a run.

use super::Core;
use crate::db::queries::WorkspaceRow;
use crate::governor::{self, Preflight};
use crate::providers::{self, keychain};
use crate::router::{PoolStatus, RouteEnv};
use crate::settings::Settings;
use crate::types::*;
use serde::Serialize;
use std::collections::HashMap;

/// Installed local models per runtime + whether each runtime is reachable/startable.
pub struct LocalState {
    pub ollama_up: bool,
    pub ollama_startable: bool,
    pub ollama_models: HashMap<String, f64>,
    pub ollama_loaded: Vec<String>,
}

impl Core {
    pub async fn local_state(&self, settings: &Settings) -> LocalState {
        let client = self.ollama.client();
        let up = client.version().await.is_some();
        let mut models: HashMap<String, f64> = HashMap::new();
        let mut loaded = vec![];
        if up {
            if let Ok(tags) = client.tags().await {
                for t in tags {
                    models.insert(t.name, t.size as f64 / 1_073_741_824.0);
                }
            }
            loaded = client.ps().await.unwrap_or_default().into_iter().map(|m| m.name).collect();
        } else {
            // Read the store on disk so a stopped server still shows what's installed.
            for (name, size) in providers::scan_ollama_store(&crate::governor::service::OllamaService::models_dir(settings)) {
                models.insert(name, size as f64 / 1_073_741_824.0);
            }
        }
        LocalState {
            ollama_up: up,
            ollama_startable: settings.manage_ollama && crate::proc::which("ollama").is_some(),
            ollama_models: models,
            ollama_loaded: loaded,
        }
    }

    fn model_installed(ls: &LocalState, name: &str) -> Option<f64> {
        ls.ollama_models
            .get(name)
            .or_else(|| ls.ollama_models.get(&format!("{name}:latest")))
            .or_else(|| ls.ollama_models.get(name.trim_end_matches(":latest")))
            .copied()
    }

    /// Why an executor-pool entry can't be used right now (`None` = usable).
    pub async fn entry_block_reason(
        &self,
        run_id: &str,
        e: &ExecutorEntry,
        ws: Option<&WorkspaceRow>,
        settings: &Settings,
        ls: &LocalState,
        sys: &governor::SysView,
    ) -> Option<String> {
        let infos = self.agent_infos();
        let Some(agent) = infos.iter().find(|a| a.id == e.agent_id) else { return Some("unknown agent".into()) };
        if !agent.installed {
            return Some(format!("{} isn't installed", agent.display_name));
        }
        if !agent.enabled {
            return Some(format!("{} is turned off", agent.display_name));
        }
        if !agent.providers.contains(&e.model.provider_type) {
            return Some(format!("{} can't drive {} models", agent.display_name, e.model.provider_type));
        }
        let m = &e.model;
        if m.tier == Tier::Local {
            match m.provider_type {
                ProviderType::Ollama => {
                    if !ls.ollama_up && !ls.ollama_startable {
                        return Some("Ollama isn't running".into());
                    }
                    let Some(size) = Self::model_installed(ls, &m.name) else {
                        return Some(format!("{} isn't downloaded", m.label()));
                    };
                    let loaded = ls.ollama_loaded.iter().any(|l| l == &m.name);
                    return self.governor.local_block_reason(run_id, m, Some(size), loaded, settings, sys);
                }
                ProviderType::Fake => return self.governor.local_block_reason(run_id, m, None, true, settings, sys),
                _ => {
                    let base = crate::adapters::opencode::openai_base(m);
                    let rt = providers::detect_openai_runtime(m.provider_type, &base).await;
                    if !rt.running {
                        return Some(format!("{} isn't running at {base}", m.provider_type));
                    }
                    return self.governor.local_block_reason(run_id, m, None, true, settings, sys);
                }
            }
        }
        // Cloud
        if ws.is_some_and(|w| w.local_only) {
            return Some("this workspace is local only".into());
        }
        if m.provider_type == ProviderType::Fake {
            return None;
        }
        let providers = self.db.providers().unwrap_or_default();
        let prov = providers.iter().find(|p| p.id == m.provider_id);
        if prov.is_some_and(|p| !p.enabled) {
            return Some("provider is turned off".into());
        }
        if let Some(cap) = prov.and_then(|p| p.monthly_cap_usd) {
            let spent = self.db.provider_spend_since(&m.provider_id, month_start()).unwrap_or(0.0);
            if spent >= cap {
                return Some(format!("monthly cap of ${cap:.2} reached (~${spent:.2} spent)"));
            }
        }
        if let Some(k) = &m.key_ref {
            if keychain::get(k).is_none() {
                return Some("API key missing from the Keychain".into());
            }
        }
        None
    }

    pub async fn build_env(
        &self,
        run_id: &str,
        ws: Option<&WorkspaceRow>,
        mode: &ModeDef,
        settings: &Settings,
        pool_override: Option<&[ExecutorEntry]>,
    ) -> RouteEnv {
        let ls = self.local_state(settings).await;
        let sys = governor::sys_view();
        let pool_src: Vec<ExecutorEntry> = pool_override.map(|p| p.to_vec()).unwrap_or_else(|| settings.executor_pool.clone());
        let mut pool = vec![];
        for e in pool_src {
            let blocked =
                if e.enabled { self.entry_block_reason(run_id, &e, ws, settings, &ls, &sys).await } else { Some("turned off".into()) };
            pool.push(PoolStatus { entry: e, blocked });
        }
        let usable = self.usable_agents();
        let paid_agent = settings
            .planner_agent
            .clone()
            .filter(|id| usable.iter().any(|a| &a.id == id && a.family == AgentFamily::Paid))
            .or_else(|| usable.iter().find(|a| a.family == AgentFamily::Paid).map(|a| a.id.clone()));
        let budget_planner = if settings.budget_planning {
            match &settings.budget_planner {
                Some(bp) => match self.entry_block_reason(run_id, bp, ws, settings, &ls, &sys).await {
                    None => Some(bp.clone()),
                    Some(why) => {
                        self.bus.notice("warn", format!("Budget planner unavailable ({why}); using the paid agent."), Some(run_id));
                        None
                    }
                },
                None => None,
            }
        } else {
            None
        };
        RouteEnv { mode: mode.clone(), paid_agent, budget_planner, pool }
    }

    /// Pre-flight for the Home screen: memory fit of the local model a run
    /// would load, system notes, and alternatives when it's tight.
    pub async fn preflight(&self, ws: Option<&WorkspaceRow>, pool_override: Option<&[ExecutorEntry]>) -> PreflightView {
        let settings = self.db.settings();
        let ls = self.local_state(&settings).await;
        let sys = governor::sys_view();
        let pool: Vec<ExecutorEntry> = pool_override.map(|p| p.to_vec()).unwrap_or_else(|| settings.executor_pool.clone());
        let first_local = pool.iter().find(|e| e.enabled && e.model.tier == Tier::Local && e.model.provider_type == ProviderType::Ollama);
        let pf = match first_local {
            Some(e) => {
                let size = Self::model_installed(&ls, &e.model.name);
                let loaded = ls.ollama_loaded.iter().any(|l| l == &e.model.name);
                governor::preflight(Some((&e.model, size)), loaded, &settings, sys.clone())
            }
            None => governor::preflight(None, false, &settings, sys.clone()),
        };
        let budget = pf.budget_gb;
        let mut smaller = vec![];
        if let (Some(e), Some(need)) = (first_local, pf.mem_needed_gb) {
            for (name, size) in &ls.ollama_models {
                if name == &e.model.name {
                    continue;
                }
                let mem = governor::estimate_mem_gb(*size, governor::choose_ctx(*size, sys.total_gb, budget));
                if mem < need && governor::fit(mem, sys.free_gb, budget, false) == governor::Fit::Fits {
                    let mut m = e.model.clone();
                    m.name = name.clone();
                    m.display_name = None;
                    m.mem_needed_gb = Some(mem);
                    smaller.push(ExecutorEntry { agent_id: e.agent_id.clone(), model: m, enabled: true });
                }
            }
        }
        let cloud: Vec<ExecutorEntry> = if ws.is_some_and(|w| w.local_only) {
            vec![]
        } else {
            pool.iter().filter(|e| e.enabled && e.model.tier == Tier::CheapCloud).cloned().collect()
        };
        let paid = self.usable_agents().into_iter().find(|a| a.family == AgentFamily::Paid).map(|a| a.display_name);
        PreflightView { preflight: pf, smaller, cloud, paid_agent: paid, pool_empty: pool.iter().all(|e| !e.enabled) }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct PreflightView {
    #[serde(flatten)]
    pub preflight: Preflight,
    /// Smaller installed local models that fit right now.
    pub smaller: Vec<ExecutorEntry>,
    /// Cloud entries that could run this instead.
    pub cloud: Vec<ExecutorEntry>,
    pub paid_agent: Option<String>,
    pub pool_empty: bool,
}

/// Start of the current calendar month (UTC), in ms.
pub fn month_start() -> i64 {
    let now = crate::db::now_ms() / 1000;
    let days = now / 86_400;
    // Civil-from-days (Howard Hinnant) to find the 1st of this month.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    (days - (d - 1)) * 86_400_000
}

#[cfg(test)]
mod tests {
    #[test]
    fn month_start_is_in_the_past_month() {
        let m = super::month_start();
        let now = crate::db::now_ms();
        assert!(m <= now && now - m < 32 * 86_400_000);
    }
}
