//! Resource sampler. Runs every 2 s only while a run is active or the
//! Resource view is open (reference-counted); otherwise it's off. It refreshes
//! only the process trees the app started, never a full system scan.

use super::{Bus, UiEvent};
use crate::macos::{self, Pressure, PowerSource, Thermal};
use crate::proc::ProcRegistry;
use crate::providers::Ollama;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Debug, Default)]
pub struct SysTotals {
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub pressure: Option<Pressure>,
    pub thermal: Option<Thermal>,
    pub power: Option<PowerSource>,
    pub low_power: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProcUsage {
    pub label: String,
    pub owner: String,
    pub pid: u32,
    pub rss_mb: f64,
    pub cpu_pct: f64,
    pub processes: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelUsage {
    pub name: String,
    pub mem_mb: f64,
    pub vram_mb: f64,
    pub expires_at: String,
    pub loaded_by_app: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Snapshot {
    pub ts: i64,
    pub sys: SysTotals,
    pub app: Option<ProcUsage>,
    pub procs: Vec<ProcUsage>,
    pub models: Vec<ModelUsage>,
    pub running: Vec<String>,
    pub cost_usd: f64,
    /// Memory used by agents + models (MB).
    pub controlled_mb: f64,
    pub sampling: bool,
}

pub trait SamplerHooks: Send + Sync {
    /// Labels of what's running ("OpenCode · qwen2.5-coder:7b") and the live cost estimate.
    fn running(&self) -> (Vec<String>, f64);
    fn loaded_by_app(&self) -> HashSet<String>;
    fn ollama(&self) -> Option<Ollama>;
    fn on_snapshot(&self, s: &Snapshot);
}

pub struct Sampler {
    registry: ProcRegistry,
    bus: Bus,
    db: crate::db::Db,
    hooks: Mutex<Option<Arc<dyn SamplerHooks>>>,
    refs: Mutex<usize>,
    sys: Mutex<System>,
}

impl Sampler {
    pub fn new(registry: ProcRegistry, bus: Bus, db: crate::db::Db) -> Arc<Self> {
        Arc::new(Self { registry, bus, db, hooks: Mutex::new(None), refs: Mutex::new(0), sys: Mutex::new(System::new()) })
    }

    pub fn set_hooks(&self, h: Arc<dyn SamplerHooks>) {
        *self.hooks.lock() = Some(h);
    }

    /// Starts sampling (if not already) until the guard's `release` is called.
    pub fn acquire(self: &Arc<Self>) {
        let start = {
            let mut r = self.refs.lock();
            *r += 1;
            *r == 1
        };
        if start {
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    if *me.refs.lock() == 0 {
                        break;
                    }
                    let snap = me.sample(true).await;
                    me.bus.send(UiEvent::Resources { snapshot: snap });
                    tokio::time::sleep(INTERVAL).await;
                }
                // One last idle snapshot so the UI shows the settled state.
                let snap = me.sample(false).await;
                me.bus.send(UiEvent::Resources { snapshot: snap });
            });
        }
    }

    pub fn release(&self) {
        let mut r = self.refs.lock();
        *r = r.saturating_sub(1);
    }

    pub fn active(&self) -> bool {
        *self.refs.lock() > 0
    }

    /// One snapshot. `record` writes metrics rows (only while sampling).
    pub async fn sample(&self, record: bool) -> Snapshot {
        let hooks = self.hooks.lock().clone();
        let tracked = self.registry.list();
        let me = std::process::id();

        // Build process trees off the async runtime threads (libproc calls).
        let roots: Vec<(String, String, u32)> = tracked.iter().map(|p| (p.label.clone(), p.owner.clone(), p.pid)).collect();
        let trees: Vec<(String, String, u32, Vec<u32>)> = tokio::task::spawn_blocking(move || {
            let mut v: Vec<(String, String, u32, Vec<u32>)> =
                roots.into_iter().map(|(l, o, p)| (l, o, p, macos::process_tree(p))).collect();
            v.push(("This app".into(), "app".into(), me, vec![me]));
            v
        })
        .await
        .unwrap_or_default();

        let (procs, app, totals) = {
            let mut sys = self.sys.lock();
            let all: Vec<Pid> = trees.iter().flat_map(|t| t.3.iter().map(|p| Pid::from_u32(*p))).collect();
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&all), true, ProcessRefreshKind::nothing().with_memory().with_cpu());
            sys.refresh_memory();
            let mut procs = vec![];
            let mut app = None;
            for (label, owner, pid, tree) in &trees {
                let (mut rss, mut cpu) = (0u64, 0f32);
                let mut n = 0;
                for p in tree {
                    if let Some(pr) = sys.process(Pid::from_u32(*p)) {
                        rss += pr.memory();
                        cpu += pr.cpu_usage();
                        n += 1;
                    }
                }
                let u = ProcUsage {
                    label: label.clone(),
                    owner: owner.clone(),
                    pid: *pid,
                    rss_mb: rss as f64 / 1_048_576.0,
                    cpu_pct: cpu as f64,
                    processes: n,
                };
                if owner == "app" {
                    app = Some(u);
                } else {
                    procs.push(u);
                }
            }
            let gb = 1_073_741_824.0;
            let total = sys.total_memory() as f64 / gb;
            let avail = sys.available_memory() as f64 / gb;
            (procs, app, (total, avail))
        };

        let loaded = hooks.as_ref().map(|h| h.loaded_by_app()).unwrap_or_default();
        let mut models = vec![];
        if let Some(o) = hooks.as_ref().and_then(|h| h.ollama()) {
            if let Ok(ps) = o.ps().await {
                for m in ps {
                    models.push(ModelUsage {
                        loaded_by_app: loaded.contains(&m.name),
                        mem_mb: m.size as f64 / 1_048_576.0,
                        vram_mb: m.size_vram as f64 / 1_048_576.0,
                        expires_at: m.expires_at,
                        name: m.name,
                    });
                }
            }
        }
        let (running, cost) = hooks.as_ref().map(|h| h.running()).unwrap_or_default();
        let ollama_proc_mb: f64 = procs.iter().filter(|p| p.owner == "ollama").map(|p| p.rss_mb).sum();
        let agents_mb: f64 = procs.iter().filter(|p| p.owner != "ollama").map(|p| p.rss_mb).sum();
        let models_mb: f64 = models.iter().map(|m| m.mem_mb).sum::<f64>().max(ollama_proc_mb);
        let snap = Snapshot {
            ts: crate::db::now_ms(),
            sys: SysTotals {
                total_gb: round1(totals.0),
                used_gb: round1(totals.0 - totals.1),
                free_gb: round1(totals.1),
                pressure: Some(macos::pressure_now()),
                thermal: Some(macos::thermal_now()),
                power: Some(macos::power_source()),
                low_power: macos::low_power_mode(),
            },
            app,
            procs,
            models,
            running,
            cost_usd: cost,
            controlled_mb: agents_mb + models_mb,
            sampling: record,
        };
        if record {
            let w = self.db.writer();
            for p in snap.procs.iter().chain(snap.app.iter()) {
                w.send(crate::db::WriteOp::Metric { ts: snap.ts, target: p.label.clone(), rss_mb: p.rss_mb, cpu_pct: p.cpu_pct, vram_mb: None });
            }
            for m in &snap.models {
                w.send(crate::db::WriteOp::Metric { ts: snap.ts, target: format!("model:{}", m.name), rss_mb: m.mem_mb, cpu_pct: 0.0, vram_mb: Some(m.vram_mb) });
            }
        }
        if let Some(h) = hooks {
            h.on_snapshot(&snap);
        }
        snap
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
