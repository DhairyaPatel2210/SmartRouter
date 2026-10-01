//! The resource governor owns every decision that affects the Mac's memory,
//! CPU, heat and battery: memory budget, pre-flight fit, one local model at a
//! time, idle unload, and responses to pressure, thermal and battery state.
//! Every action produces a plain-language reason.

pub mod service;

use crate::macos::{self, PowerSource, Pressure, Thermal};
use crate::settings::Settings;
use crate::types::{ModelRef, ProviderType, Tier};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Fits,
    Tight,
    WontFit,
}

impl Fit {
    pub fn label(&self) -> &'static str {
        match self {
            Fit::Fits => "Fits",
            Fit::Tight => "Tight",
            Fit::WontFit => "Won't fit",
        }
    }
}

/// Default share of unified memory local models may use.
pub fn budget_pct(total_gb: f64, setting: Option<u8>) -> u8 {
    setting.unwrap_or(if total_gb <= 16.5 { 60 } else { 70 }).clamp(20, 90)
}

pub fn budget_gb(total_gb: f64, setting: Option<u8>) -> f64 {
    total_gb * budget_pct(total_gb, setting) as f64 / 100.0
}

/// Memory a GGUF model needs at a context length: weights plus KV cache,
/// scaled by model size. An estimate, always shown with "~".
pub fn estimate_mem_gb(download_gb: f64, ctx: u32) -> f64 {
    let kv = (ctx as f64 / 1024.0) * 0.05 * (download_gb / 4.5).max(0.5);
    ((download_gb * 1.1 + kv + 0.3) * 10.0).round() / 10.0
}

/// Largest standard context that keeps the model inside the budget
/// (default 16k on ≤16 GB Macs, 32k above).
pub fn choose_ctx(download_gb: f64, total_gb: f64, budget: f64) -> u32 {
    let cap = if total_gb <= 16.5 { 16_384 } else { 32_768 };
    for c in [32_768u32, 16_384, 8_192, 4_096] {
        if c <= cap && estimate_mem_gb(download_gb, c) <= budget {
            return c;
        }
    }
    4_096
}

pub fn fit(mem_needed: f64, free_gb: f64, budget: f64, already_loaded: bool) -> Fit {
    if mem_needed > budget {
        return Fit::WontFit;
    }
    if already_loaded {
        return Fit::Fits;
    }
    let after = free_gb - mem_needed;
    if after < 0.0 {
        Fit::WontFit
    } else if after < 2.0 || mem_needed > budget * 0.85 {
        Fit::Tight
    } else {
        Fit::Fits
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct SysView {
    pub total_gb: f64,
    pub free_gb: f64,
    pub pressure: Pressure,
    pub thermal: Thermal,
    pub power: PowerSource,
    pub low_power: bool,
}

/// A cheap on-demand read of system state (memory + native signals).
pub fn sys_view() -> SysView {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    let gb = 1_073_741_824.0;
    SysView {
        total_gb: s.total_memory() as f64 / gb,
        free_gb: s.available_memory() as f64 / gb,
        pressure: macos::pressure_now(),
        thermal: macos::thermal_now(),
        power: macos::power_source(),
        low_power: macos::low_power_mode(),
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Preflight {
    pub fit: Option<Fit>,
    /// One-line summary, e.g. "Loads Qwen Coder 14B (~9.5 GB). You have 11 GB free, so ~1.5 GB after load: Tight."
    pub line: String,
    pub model: Option<String>,
    pub mem_needed_gb: Option<f64>,
    pub free_gb: f64,
    pub budget_gb: f64,
    pub notes: Vec<String>,
    pub sys: SysView,
}

fn gb(x: f64) -> String {
    if x >= 10.0 {
        format!("{x:.0} GB")
    } else {
        format!("{x:.1} GB")
    }
}

/// Memory a local model needs (from the catalog/DB or estimated from size).
pub fn model_mem(m: &ModelRef, size_gb: Option<f64>) -> Option<f64> {
    m.mem_needed_gb.or_else(|| size_gb.map(|s| estimate_mem_gb(s, m.ctx_len.unwrap_or(16_384))))
}

pub fn preflight(local: Option<(&ModelRef, Option<f64>)>, already_loaded: bool, settings: &Settings, sys: SysView) -> Preflight {
    let budget = budget_gb(sys.total_gb, settings.memory_budget_pct);
    let mut notes = vec![];
    if sys.pressure == Pressure::Critical {
        notes.push("Memory pressure is critical right now; local steps will wait or move to cloud.".into());
    } else if sys.pressure == Pressure::Warning {
        notes.push("Memory pressure is elevated; close heavy apps for smoother local runs.".into());
    }
    if sys.thermal >= Thermal::Serious {
        notes.push("Your Mac is running hot; cheap steps prefer cloud until it cools.".into());
    }
    if sys.power == PowerSource::Battery && settings.prefer_cloud_on_battery {
        notes.push("On battery: cheap steps prefer cloud (Settings → Resources).".into());
    }
    if sys.low_power {
        notes.push("Low Power Mode is on; local models will be slower.".into());
    }
    let Some((m, size)) = local else {
        return Preflight {
            fit: None,
            line: "No local model in this run: cheap steps use cloud, so your Mac stays light.".into(),
            model: None,
            mem_needed_gb: None,
            free_gb: sys.free_gb,
            budget_gb: budget,
            notes,
            sys,
        };
    };
    let need = model_mem(m, size).unwrap_or(5.0);
    let f = fit(need, sys.free_gb, budget, already_loaded);
    let line = if already_loaded {
        format!("{} is already loaded (~{}). Fits.", m.label(), gb(need))
    } else if need > budget {
        format!(
            "Loads {} (~{}), over your local-model budget of {} ({}% of {}): Won't fit.",
            m.label(),
            gb(need),
            gb(budget),
            budget_pct(sys.total_gb, settings.memory_budget_pct),
            gb(sys.total_gb)
        )
    } else {
        format!(
            "Loads {} (~{}). You have {} free, so ~{} after load: {}.",
            m.label(),
            gb(need),
            gb(sys.free_gb),
            gb((sys.free_gb - need).max(0.0)),
            f.label()
        )
    };
    Preflight {
        fit: Some(f),
        line,
        model: Some(m.name.clone()),
        mem_needed_gb: Some(need),
        free_gb: sys.free_gb,
        budget_gb: budget,
        notes,
        sys,
    }
}

#[derive(Default)]
pub struct GovState {
    pub pressure: Option<Pressure>,
    /// Models the app loaded (unloaded on Stop everything / run end).
    pub loaded_by_app: HashSet<String>,
    /// Runs currently executing a local step.
    pub local_runs: HashSet<String>,
}

#[derive(Default)]
pub struct Governor {
    pub state: Mutex<GovState>,
}

impl Governor {
    /// Why a local executor can't be used right now (`None` = allowed).
    pub fn local_block_reason(
        &self,
        run_id: &str,
        m: &ModelRef,
        size_gb: Option<f64>,
        loaded: bool,
        settings: &Settings,
        sys: &SysView,
    ) -> Option<String> {
        if m.tier != Tier::Local {
            return None;
        }
        let st = self.state.lock();
        if sys.pressure == Pressure::Critical {
            return Some("memory pressure is critical".into());
        }
        if sys.thermal >= Thermal::Serious && settings.prefer_cloud_when_hot {
            return Some(format!("your Mac is hot (thermal state {:?})", sys.thermal).to_lowercase());
        }
        if sys.power == PowerSource::Battery && settings.prefer_cloud_on_battery {
            return Some("on battery with \"prefer cloud on battery\" on".into());
        }
        let budget = budget_gb(sys.total_gb, settings.memory_budget_pct);
        let need = model_mem(m, size_gb).unwrap_or(5.0);
        if need > budget {
            return Some(format!("{} needs ~{} but the local-model budget is {}", m.label(), gb(need), gb(budget)));
        }
        if !loaded && fit(need, sys.free_gb, budget, false) == Fit::WontFit {
            return Some(format!("only {} free; {} needs ~{}", gb(sys.free_gb), m.label(), gb(need)));
        }
        // One local run at a time on ≤16 GB Macs.
        if sys.total_gb <= 16.5 && !st.local_runs.is_empty() && !st.local_runs.contains(run_id) {
            return Some("another run is using the local model".into());
        }
        None
    }

    pub fn enter_local(&self, run_id: &str) {
        self.state.lock().local_runs.insert(run_id.into());
    }

    pub fn leave_local(&self, run_id: &str) {
        self.state.lock().local_runs.remove(run_id);
    }

    pub fn mark_loaded(&self, model: &str) {
        self.state.lock().loaded_by_app.insert(model.into());
    }

    pub fn take_loaded(&self) -> Vec<String> {
        self.state.lock().loaded_by_app.drain().collect()
    }

    pub fn loaded(&self) -> HashSet<String> {
        self.state.lock().loaded_by_app.clone()
    }
}

/// Lower scheduling priority applies to agent CLIs, checks and the managed model server.
pub fn low_priority(settings: &Settings) -> bool {
    settings.lower_priority
}

pub fn is_local(m: &ModelRef) -> bool {
    m.tier == Tier::Local || m.provider_type.is_local() || m.provider_type == ProviderType::Ollama
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(total: f64, free: f64) -> SysView {
        SysView {
            total_gb: total,
            free_gb: free,
            pressure: Pressure::Normal,
            thermal: Thermal::Nominal,
            power: PowerSource::Ac,
            low_power: false,
        }
    }

    fn model(mem: f64) -> ModelRef {
        ModelRef {
            provider_id: "ollama".into(),
            provider_type: ProviderType::Ollama,
            name: "qwen2.5-coder:14b".into(),
            display_name: Some("Qwen Coder 14B".into()),
            base_url: None,
            key_ref: None,
            tier: Tier::Local,
            mem_needed_gb: Some(mem),
            ctx_len: None,
            price_in_per_m: None,
            price_out_per_m: None,
        }
    }

    #[test]
    fn budgets() {
        assert_eq!(budget_pct(8.0, None), 60);
        assert_eq!(budget_pct(32.0, None), 70);
        assert!((budget_gb(16.0, None) - 9.6).abs() < 1e-9);
        assert_eq!(budget_pct(16.0, Some(95)), 90);
    }

    #[test]
    fn fit_badges() {
        assert_eq!(fit(9.5, 11.0, 22.0, false), Fit::Tight);
        assert_eq!(fit(4.0, 11.0, 22.0, false), Fit::Fits);
        assert_eq!(fit(12.0, 11.0, 22.0, false), Fit::WontFit);
        assert_eq!(fit(10.0, 20.0, 9.6, false), Fit::WontFit, "over budget");
        assert_eq!(fit(9.0, 1.0, 22.0, true), Fit::Fits, "already loaded");
    }

    #[test]
    fn preflight_line_matches_spec_example() {
        let p = preflight(Some((&model(9.5), None)), false, &Settings::default(), sys(32.0, 11.0));
        assert_eq!(p.fit, Some(Fit::Tight));
        assert_eq!(p.line, "Loads Qwen Coder 14B (~9.5 GB). You have 11 GB free, so ~1.5 GB after load: Tight.");
    }

    #[test]
    fn context_is_capped_to_budget() {
        // 7B Q4 (~4.7 GB) on an 8 GB Mac: budget 4.8 GB → smallest context.
        assert_eq!(choose_ctx(4.7, 8.0, budget_gb(8.0, None)), 4_096);
        assert_eq!(choose_ctx(4.7, 16.0, budget_gb(16.0, None)), 16_384);
        assert_eq!(choose_ctx(4.7, 64.0, budget_gb(64.0, None)), 32_768);
    }

    #[test]
    fn local_gating_reasons() {
        let g = Governor::default();
        let s = Settings::default();
        assert!(g.local_block_reason("r1", &model(9.0), None, false, &s, &sys(8.0, 6.0)).unwrap().contains("budget"));
        assert!(g.local_block_reason("r1", &model(3.0), None, false, &s, &sys(16.0, 10.0)).is_none());
        let mut hot = sys(16.0, 10.0);
        hot.thermal = Thermal::Serious;
        assert!(g.local_block_reason("r1", &model(3.0), None, false, &s, &hot).unwrap().contains("hot"));
        let mut batt = sys(16.0, 10.0);
        batt.power = PowerSource::Battery;
        assert!(g.local_block_reason("r1", &model(3.0), None, false, &s, &batt).unwrap().contains("battery"));
        g.enter_local("r1");
        assert!(g.local_block_reason("r2", &model(3.0), None, false, &s, &sys(16.0, 10.0)).unwrap().contains("another run"));
        assert!(g.local_block_reason("r1", &model(3.0), None, false, &s, &sys(16.0, 10.0)).is_none());
    }
}
