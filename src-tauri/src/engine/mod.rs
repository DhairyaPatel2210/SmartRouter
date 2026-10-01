//! The orchestration core: shared state ([`Core`]), per-run control
//! ([`RunCtl`]), approvals, and the user controls (cancel, pause, mode
//! change, reassign, rollback, Stop everything). The run lifecycle itself is
//! in [`run`]; routing inputs and pre-flight in [`env`].

pub mod env;
pub mod run;

use crate::adapters::{self, AdapterCtx, AgentAdapter, AuthStatus, DetectResult};
use crate::db::queries::{AgentRow, RunRow, StepRow};
use crate::db::{now_ms, Db};
use crate::governor::{service::OllamaService, Governor};
use crate::installer::Catalog;
use crate::macos::Pressure;
use crate::proc::{CancelToken, ProcRegistry};
use crate::settings::DirtyStrategy;
use crate::telemetry::sampler::{Sampler, SamplerHooks, Snapshot};
use crate::telemetry::{Bus, Emitter, Logs, UiEvent};
use crate::types::*;
use anyhow::{anyhow, bail, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use tokio::sync::{oneshot, Notify, Semaphore};

#[derive(Serialize, Clone, Debug)]
pub struct ApprovalOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub danger: bool,
}

impl ApprovalOption {
    pub fn new(id: &str, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), primary: false, danger: false }
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ApprovalRequest {
    pub id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    /// paid | cloud | cloud_notice | escalate | failed | budget | pressure | scope
    pub kind: String,
    pub title: String,
    pub body: String,
    pub options: Vec<ApprovalOption>,
}

/// A user's manual assignment for a pending step.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Assignment {
    pub agent_id: String,
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub library_agent: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct StartOpts {
    pub workspace_id: String,
    pub goal: String,
    pub mode: Option<String>,
    #[serde(default)]
    pub dirty_strategy: Option<DirtyStrategy>,
    /// Initialise git in a non-git folder (recommended; enables per-step rollback).
    #[serde(default)]
    pub init_git: bool,
    /// Executor pool for this run only (pre-flight "use cloud" / "smaller model").
    #[serde(default)]
    pub pool_override: Option<Vec<ExecutorEntry>>,
}

pub struct RunCtl {
    pub run_id: String,
    pub workspace_id: String,
    pub ws: PathBuf,
    pub cancel: CancelToken,
    pub step_cancel: Mutex<Option<CancelToken>>,
    pub paused: AtomicBool,
    pub resume: Notify,
    pub mode: Mutex<String>,
    pub overrides: Mutex<HashMap<String, Assignment>>,
    pending: Mutex<HashMap<String, (ApprovalRequest, oneshot::Sender<String>)>>,
    /// Why the running step was interrupted (memory pressure, scope guard).
    pub interrupt: Mutex<Option<String>>,
    pub cost: Mutex<f64>,
    pub label: Mutex<Option<String>>,
    pub local_step: AtomicBool,
    pub peak_mb: Mutex<f64>,
}

impl RunCtl {
    pub fn mode(&self) -> String {
        self.mode.lock().clone()
    }

    /// Waits while paused (or until cancelled).
    pub async fn wait_if_paused(&self) {
        while self.paused.load(Ordering::SeqCst) && !self.cancel.is_cancelled() {
            tokio::select! {
                _ = self.resume.notified() => {}
                _ = self.cancel.cancelled() => {}
            }
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct AgentInfo {
    pub id: String,
    pub display_name: String,
    pub family: AgentFamily,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub auth: Option<AuthStatus>,
    pub providers: Vec<ProviderType>,
    pub login_command: Option<String>,
    pub enabled: bool,
    pub tagline: String,
    pub docs: String,
    pub known_good_version: String,
}

/// Shows a notification (title, body).
pub type Notifier = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct CoreDeps {
    pub data_dir: PathBuf,
    pub catalog_dir: PathBuf,
    pub emitter: Arc<dyn Emitter>,
    pub demo_agents: bool,
    /// Shows a macOS notification (title, body) when the window is hidden.
    pub notifier: Notifier,
}

pub struct Core {
    pub db: Db,
    pub bus: Bus,
    pub logs: Logs,
    pub registry: ProcRegistry,
    pub data_dir: PathBuf,
    pub governor: Governor,
    pub ollama: OllamaService,
    pub sampler: Arc<Sampler>,
    pub catalog: Catalog,
    adapters: RwLock<Vec<Arc<dyn AgentAdapter>>>,
    detect: RwLock<HashMap<String, (DetectResult, Option<AuthStatus>)>>,
    runs: Mutex<HashMap<String, Arc<RunCtl>>>,
    slots: Arc<Semaphore>,
    notifier: Notifier,
    pub window_visible: AtomicBool,
    pub resource_view_open: AtomicBool,
    pub pulls: Mutex<HashMap<String, CancelToken>>,
    pub jobs: Mutex<HashMap<String, CancelToken>>,
}

struct Hooks(Weak<Core>);

impl SamplerHooks for Hooks {
    fn running(&self) -> (Vec<String>, f64) {
        let Some(c) = self.0.upgrade() else { return Default::default() };
        let runs = c.runs.lock();
        let labels = runs.values().filter_map(|r| r.label.lock().clone()).collect();
        let cost = runs.values().map(|r| *r.cost.lock()).sum();
        (labels, cost)
    }
    fn loaded_by_app(&self) -> HashSet<String> {
        self.0.upgrade().map(|c| c.governor.loaded()).unwrap_or_default()
    }
    fn ollama(&self) -> Option<crate::providers::Ollama> {
        self.0.upgrade().map(|c| c.ollama.client())
    }
    fn on_snapshot(&self, s: &Snapshot) {
        let Some(c) = self.0.upgrade() else { return };
        for r in c.runs.lock().values() {
            let mut p = r.peak_mb.lock();
            if s.controlled_mb > *p {
                *p = s.controlled_mb;
            }
        }
    }
}

impl Core {
    pub fn new(deps: CoreDeps) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&deps.data_dir)?;
        let db = Db::open(&deps.data_dir.join("orchestrator.db"))?;
        let orphans = db.fail_orphan_runs()?;
        if orphans > 0 {
            log::warn!("marked {orphans} interrupted runs as failed");
        }
        let _ = db.prune_metrics(30);
        let settings = db.settings();
        let bus = Bus::new(deps.emitter);
        let registry = ProcRegistry::default();
        let sampler = Sampler::new(registry.clone(), bus.clone(), db.clone());
        let catalog = Catalog::load(&deps.catalog_dir).unwrap_or_default();
        let _ = crate::library::install_starter(&deps.data_dir, &catalog.starter_dir());
        let ctx = AdapterCtx { registry: registry.clone(), data_dir: deps.data_dir.clone(), demo_agents: deps.demo_agents };
        let core = Arc::new(Self {
            logs: Logs::new(&deps.data_dir, settings.log_ring_lines),
            adapters: RwLock::new(adapters::registry(&ctx)),
            detect: RwLock::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(settings.global_concurrency.max(1) as usize)),
            governor: Governor::default(),
            ollama: OllamaService::default(),
            data_dir: deps.data_dir,
            notifier: deps.notifier,
            window_visible: AtomicBool::new(true),
            resource_view_open: AtomicBool::new(false),
            pulls: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            db,
            bus,
            registry,
            sampler,
            catalog,
        });
        core.sampler.set_hooks(Arc::new(Hooks(Arc::downgrade(&core))));
        Ok(core)
    }

    pub fn notify(&self, title: &str, body: &str) {
        if !self.window_visible.load(Ordering::SeqCst) && self.db.settings().notifications {
            (self.notifier)(title, body);
        }
    }

    // ---------- agents ----------

    pub fn adapter(&self, id: &str) -> Option<Arc<dyn AgentAdapter>> {
        self.adapters.read().iter().find(|a| a.id() == id).cloned()
    }

    pub fn adapters(&self) -> Vec<Arc<dyn AgentAdapter>> {
        self.adapters.read().clone()
    }

    /// Detects every adapter in parallel (version, path) and, if `auth`, checks login.
    pub async fn refresh_agents(&self, auth: bool) -> Vec<AgentInfo> {
        let list = self.adapters();
        let futs = list.iter().map(|a| {
            let a = a.clone();
            async move {
                let d = a.detect().await;
                let au = if auth && d.installed { Some(a.check_auth().await) } else { None };
                (a.id().to_string(), d, au)
            }
        });
        let results = futures_util::future::join_all(futs).await;
        {
            let mut det = self.detect.write();
            for (id, d, au) in results {
                let prev_auth = det.get(&id).and_then(|x| x.1.clone());
                det.insert(id, (d, au.or(prev_auth)));
            }
        }
        let infos = self.agent_infos();
        for i in &infos {
            let _ = self.db.upsert_agent(&AgentRow {
                id: i.id.clone(),
                display_name: i.display_name.clone(),
                kind: i.family.to_string(),
                cli: self.catalog.agent(&i.id).map(|c| c.bin.clone()).unwrap_or_else(|| i.id.clone()),
                version: i.version.clone(),
                install_path: i.path.clone(),
                auth_ok: matches!(i.auth, Some(AuthStatus::Ok)),
                enabled: i.enabled,
                supports_local_models: i.providers.iter().any(|p| p.is_local()),
            });
        }
        self.agent_infos()
    }

    pub fn agent_infos(&self) -> Vec<AgentInfo> {
        let det = self.detect.read();
        let rows: HashMap<String, AgentRow> = self.db.agents().unwrap_or_default().into_iter().map(|r| (r.id.clone(), r)).collect();
        self.adapters()
            .iter()
            .map(|a| {
                let (d, au) = det.get(a.id()).cloned().unwrap_or_default();
                let row = rows.get(a.id());
                let cat = self.catalog.agent(a.id());
                AgentInfo {
                    id: a.id().into(),
                    display_name: row.map(|r| r.display_name.clone()).unwrap_or_else(|| a.display_name().into()),
                    family: a.family(),
                    installed: d.installed,
                    version: d.version,
                    path: d.path,
                    auth: au,
                    providers: a.supported_providers().to_vec(),
                    login_command: a.login_command(),
                    enabled: row.map(|r| r.enabled).unwrap_or(true),
                    tagline: cat.map(|c| c.tagline.clone()).unwrap_or_default(),
                    docs: cat.map(|c| c.docs.clone()).unwrap_or_default(),
                    known_good_version: cat.map(|c| c.known_good_version.clone()).unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Installed and enabled agents.
    pub fn usable_agents(&self) -> Vec<AgentInfo> {
        self.agent_infos().into_iter().filter(|a| a.installed && a.enabled).collect()
    }

    /// CLIs that receive Library sync (installed, enabled, real).
    pub fn sync_targets(&self) -> Vec<String> {
        self.usable_agents().into_iter().map(|a| a.id).filter(|id| crate::library::sync::CLIS.contains(&id.as_str())).collect()
    }

    // ---------- runs ----------

    pub fn active_runs(&self) -> Vec<Arc<RunCtl>> {
        self.runs.lock().values().cloned().collect()
    }

    pub fn run_ctl(&self, run_id: &str) -> Option<Arc<RunCtl>> {
        self.runs.lock().get(run_id).cloned()
    }

    pub fn start_run(self: &Arc<Self>, opts: StartOpts) -> Result<String> {
        let ws = self.db.workspace(&opts.workspace_id)?.ok_or_else(|| anyhow!("workspace not found"))?;
        if opts.goal.trim().is_empty() {
            bail!("Describe what you want done first.");
        }
        if self.runs.lock().values().any(|r| r.workspace_id == ws.id) {
            bail!("This workspace already has an active run. Wait for it to finish or cancel it.");
        }
        let settings = self.db.settings();
        let mode = opts.mode.clone().or(ws.default_mode.clone()).unwrap_or(settings.default_mode.clone());
        let id = uuid::Uuid::new_v4().to_string();
        let run = RunRow {
            id: id.clone(),
            workspace_id: ws.id.clone(),
            goal: opts.goal.trim().to_string(),
            mode: mode.clone(),
            status: RunStatus::Pending.to_string(),
            started_at: now_ms(),
            ..Default::default()
        };
        self.db.insert_run(&run)?;
        let ctl = Arc::new(RunCtl {
            run_id: id.clone(),
            workspace_id: ws.id.clone(),
            ws: PathBuf::from(&ws.path),
            cancel: CancelToken::default(),
            step_cancel: Mutex::new(None),
            paused: AtomicBool::new(false),
            resume: Notify::new(),
            mode: Mutex::new(mode),
            overrides: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            interrupt: Mutex::new(None),
            cost: Mutex::new(0.0),
            label: Mutex::new(None),
            local_step: AtomicBool::new(false),
            peak_mb: Mutex::new(0.0),
        });
        self.runs.lock().insert(id.clone(), ctl.clone());
        self.bus.send(UiEvent::Run { run });
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            run::execute(core, ctl, opts).await;
        });
        Ok(id)
    }

    pub(crate) fn forget_run(&self, run_id: &str) {
        self.runs.lock().remove(run_id);
    }

    pub fn slots(&self) -> Arc<Semaphore> {
        self.slots.clone()
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<()> {
        let ctl = self.run_ctl(run_id).ok_or_else(|| anyhow!("run is not active"))?;
        ctl.cancel.cancel();
        if let Some(c) = ctl.step_cancel.lock().as_ref() {
            c.cancel();
        }
        // Unblock any pending approvals.
        let pending: Vec<_> = ctl.pending.lock().drain().collect();
        for (id, (_, tx)) in pending {
            let _ = tx.send("stop".into());
            self.bus.send(UiEvent::ApprovalDone { id });
        }
        Ok(())
    }

    pub fn pause_run(&self, run_id: &str, paused: bool) -> Result<()> {
        let ctl = self.run_ctl(run_id).ok_or_else(|| anyhow!("run is not active"))?;
        ctl.paused.store(paused, Ordering::SeqCst);
        if !paused {
            ctl.resume.notify_waiters();
        }
        let text = if paused { "Paused after the current step finishes." } else { "Resumed." };
        self.bus.notice("info", text, Some(run_id));
        Ok(())
    }

    /// Changes the mode for the remaining steps and re-routes pending ones.
    pub async fn set_run_mode(self: &Arc<Self>, run_id: &str, mode: &str) -> Result<()> {
        let ctl = self.run_ctl(run_id).ok_or_else(|| anyhow!("run is not active"))?;
        *ctl.mode.lock() = mode.to_string();
        if let Some(mut r) = self.db.run(run_id)? {
            r.mode = mode.to_string();
            self.db.save_run(&r)?;
            self.bus.send(UiEvent::Run { run: r });
        }
        run::reroute_pending(self, &ctl).await;
        let name = self.db.settings().mode(mode).display_name;
        self.bus.notice("info", format!("Mode changed to {name} for the remaining steps."), Some(run_id));
        Ok(())
    }

    /// Reassigns a pending step to a different agent/model/Library agent.
    pub fn reassign(&self, run_id: &str, step_id: &str, a: Assignment) -> Result<StepRow> {
        let ctl = self.run_ctl(run_id).ok_or_else(|| anyhow!("run is not active"))?;
        let mut step = self.db.steps(run_id)?.into_iter().find(|s| s.id == step_id).ok_or_else(|| anyhow!("step not found"))?;
        if step.status != StepStatus::Pending.as_str() && step.status != StepStatus::AwaitingApproval.as_str() {
            bail!("Only pending steps can be reassigned.");
        }
        let adapter = self.adapter(&a.agent_id).ok_or_else(|| anyhow!("unknown agent"))?;
        if let Some(m) = &a.model {
            if adapter.family() == AgentFamily::OpenSource && !adapter.can_drive(m.provider_type) {
                bail!("{} can't drive {} models", adapter.display_name(), m.provider_type);
            }
        }
        step.agent_id = Some(a.agent_id.clone());
        step.model_id = a.model.as_ref().map(|m| format!("{}/{}", m.provider_id, m.name));
        step.tier = Some(a.model.as_ref().map(|m| m.tier).unwrap_or(Tier::Premium).to_string());
        step.library_agent_id = a.library_agent.clone();
        step.route_reason = Some("reassigned by you".into());
        step.detail["model"] = serde_json::to_value(&a.model).unwrap_or_default();
        ctl.overrides.lock().insert(step_id.to_string(), a);
        self.db.save_step(&step)?;
        self.bus.send(UiEvent::Step { step: step.clone() });
        Ok(step)
    }

    // ---------- approvals ----------

    /// Asks the user and waits for an answer (or run cancel → "stop").
    pub async fn ask(&self, ctl: &RunCtl, mut req: ApprovalRequest) -> String {
        req.id = uuid::Uuid::new_v4().to_string();
        req.run_id = ctl.run_id.clone();
        let (tx, rx) = oneshot::channel();
        ctl.pending.lock().insert(req.id.clone(), (req.clone(), tx));
        self.notify(&req.title, &req.body);
        let id = req.id.clone();
        self.bus.send(UiEvent::Approval { request: req });
        let answer = tokio::select! {
            a = rx => a.unwrap_or_else(|_| "stop".into()),
            _ = ctl.cancel.cancelled() => "stop".into(),
        };
        ctl.pending.lock().remove(&id);
        self.bus.send(UiEvent::ApprovalDone { id });
        answer
    }

    pub fn answer(&self, approval_id: &str, option: &str) -> Result<()> {
        for ctl in self.active_runs() {
            if let Some((_, tx)) = ctl.pending.lock().remove(approval_id) {
                let _ = tx.send(option.to_string());
                self.bus.send(UiEvent::ApprovalDone { id: approval_id.to_string() });
                return Ok(());
            }
        }
        bail!("that question is no longer waiting for an answer")
    }

    pub fn pending_approvals(&self) -> Vec<ApprovalRequest> {
        self.active_runs().iter().flat_map(|c| c.pending.lock().values().map(|(r, _)| r.clone()).collect::<Vec<_>>()).collect()
    }

    // ---------- resources ----------

    /// Cancels every run, kills every process tree the app started and
    /// unloads models the app loaded.
    pub async fn stop_everything(&self) -> String {
        let runs = self.active_runs();
        for r in &runs {
            let _ = self.cancel_run(&r.run_id);
        }
        let procs = self.registry.list().into_iter().filter(|p| p.owner != "ollama").count();
        self.registry.kill_all(Some("ollama")).await;
        for (_, c) in self.pulls.lock().drain() {
            c.cancel();
        }
        let freed = self.unload_app_models().await;
        let settings = self.db.settings();
        if !settings.ollama_always_on {
            self.ollama.stop(&self.registry).await;
        }
        let msg = format!(
            "Stopped {} run{}, {} process tree{}{}.",
            runs.len(),
            if runs.len() == 1 { "" } else { "s" },
            procs,
            if procs == 1 { "" } else { "s" },
            if freed > 0.05 { format!(", unloaded models ({freed:.1} GB freed)") } else { String::new() }
        );
        self.bus.notice("warn", &msg, None);
        msg
    }

    /// Unloads models the app loaded; returns GB freed (from Ollama's report).
    pub async fn unload_app_models(&self) -> f64 {
        let models = self.governor.take_loaded();
        if models.is_empty() {
            return 0.0;
        }
        let o = self.ollama.client();
        let sizes: HashMap<String, u64> = o.ps().await.unwrap_or_default().into_iter().map(|m| (m.name, m.size)).collect();
        let mut freed = 0u64;
        for m in models {
            if o.unload(&m).await.is_ok() {
                freed += sizes.get(&m).copied().unwrap_or(0);
            }
        }
        freed as f64 / 1_073_741_824.0
    }

    pub async fn unload_model(&self, name: &str) -> Result<f64> {
        let o = self.ollama.client();
        let size = o.ps().await.unwrap_or_default().into_iter().find(|m| m.name == name).map(|m| m.size).unwrap_or(0);
        o.unload(name).await?;
        self.governor.state.lock().loaded_by_app.remove(name);
        Ok(size as f64 / 1_073_741_824.0)
    }

    /// Kernel memory-pressure transitions (called from the dispatch source).
    pub async fn on_pressure(self: &Arc<Self>, level: Pressure) {
        let prev = self.governor.state.lock().pressure.replace(level);
        if prev == Some(level) {
            return;
        }
        match level {
            Pressure::Normal => {
                if prev.is_some_and(|p| p > Pressure::Normal) {
                    self.bus.notice("info", "Memory pressure is back to normal.", None);
                }
            }
            Pressure::Warning => {
                let msg = "Memory pressure is elevated. Unloading local models would free memory.";
                self.bus.send(UiEvent::Notice { level: "pressure_warning".into(), text: msg.into(), run_id: None });
                self.notify("Memory pressure warning", msg);
            }
            Pressure::Critical => {
                let mut paused = vec![];
                for r in self.active_runs() {
                    if r.local_step.load(Ordering::SeqCst) {
                        *r.interrupt.lock() = Some("memory pressure critical".into());
                        if let Some(c) = r.step_cancel.lock().as_ref() {
                            c.cancel();
                        }
                        paused.push(r.run_id.clone());
                    }
                }
                let freed = self.unload_app_models().await;
                let msg = format!(
                    "Memory pressure critical.{}{}",
                    if paused.is_empty() { String::new() } else { " Paused the local step.".into() },
                    if freed > 0.05 { format!(" Unloaded local models ({freed:.1} GB freed).") } else { String::new() }
                );
                for id in &paused {
                    self.db.writer().event(id, None, "governor", serde_json::json!({"text": msg}).to_string());
                }
                self.bus.send(UiEvent::Notice { level: "pressure_critical".into(), text: msg.clone(), run_id: paused.first().cloned() });
                self.notify("Memory pressure critical", &msg);
            }
        }
    }

    pub fn shutdown(&self) {
        for r in self.active_runs() {
            r.cancel.cancel();
        }
        let reg = self.registry.clone();
        for p in reg.list() {
            #[cfg(unix)]
            unsafe {
                libc::killpg(p.pid as i32, libc::SIGTERM);
            }
        }
        self.ollama.stop_blocking();
        self.db.flush();
    }
}
