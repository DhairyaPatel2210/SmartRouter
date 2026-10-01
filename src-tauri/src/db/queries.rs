//! Typed queries. Rows are plain serde structs sent straight to the UI.

use super::{now_ms, Db};
use crate::settings::{Settings, WorkspaceSettings};
use anyhow::Result;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

/// Parses a JSON column that must be an object (`null`/invalid → `{}`).
fn obj(s: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => serde_json::json!({}),
    }
}

/// Serializes a JSON object field, sending `{}` instead of `null` so the UI
/// can always read properties from it.
fn ser_obj<S: serde::Serializer>(v: &serde_json::Value, s: S) -> Result<S::Ok, S::Error> {
    match v {
        serde_json::Value::Object(_) => v.serialize(s),
        _ => serde_json::Map::new().serialize(s),
    }
}

// ---------- settings ----------

impl Db {
    pub fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> =
            self.with(|c| c.query_row("SELECT value_json FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional())?;
        Ok(match v {
            Some(s) => serde_json::from_str(&s).ok(),
            None => None,
        })
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let s = serde_json::to_string(value)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![key, s],
            )
        })?;
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        let mut s: Settings = self.get_setting("app").ok().flatten().unwrap_or_default();
        s.normalize();
        s
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.set_setting("app", s)
    }
}

// ---------- workspaces ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkspaceRow {
    pub id: String,
    pub path: String,
    pub display_name: String,
    pub is_git: bool,
    pub default_mode: Option<String>,
    pub library_profile_id: Option<String>,
    pub local_only: bool,
    pub last_opened_at: i64,
    pub pinned: bool,
    pub settings: WorkspaceSettings,
}

fn ws_row(r: &Row) -> rusqlite::Result<WorkspaceRow> {
    let s: String = r.get(9)?;
    Ok(WorkspaceRow {
        id: r.get(0)?,
        path: r.get(1)?,
        display_name: r.get(2)?,
        is_git: r.get(3)?,
        default_mode: r.get(4)?,
        library_profile_id: r.get(5)?,
        local_only: r.get(6)?,
        last_opened_at: r.get(7)?,
        pinned: r.get(8)?,
        settings: serde_json::from_str(&s).unwrap_or_default(),
    })
}

const WS_COLS: &str = "id, path, display_name, is_git, default_mode, library_profile_id, local_only, last_opened_at, pinned, settings_json";

impl Db {
    pub fn upsert_workspace(&self, path: &str, display_name: &str, is_git: bool) -> Result<WorkspaceRow> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        self.with(|c| {
            c.execute(
                "INSERT INTO workspaces (id, path, display_name, is_git, last_opened_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET is_git = excluded.is_git, last_opened_at = excluded.last_opened_at",
                params![id, path, display_name, is_git, now],
            )
        })?;
        self.workspace_by_path(path)?.ok_or_else(|| anyhow::anyhow!("workspace insert failed"))
    }

    pub fn workspace_by_path(&self, path: &str) -> Result<Option<WorkspaceRow>> {
        self.with(|c| c.query_row(&format!("SELECT {WS_COLS} FROM workspaces WHERE path = ?1"), [path], ws_row).optional())
    }

    pub fn workspace(&self, id: &str) -> Result<Option<WorkspaceRow>> {
        self.with(|c| c.query_row(&format!("SELECT {WS_COLS} FROM workspaces WHERE id = ?1"), [id], ws_row).optional())
    }

    /// Pinned first, then most recent; capped to the recent list size.
    pub fn recent_workspaces(&self, limit: usize) -> Result<Vec<WorkspaceRow>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {WS_COLS} FROM workspaces ORDER BY pinned DESC, last_opened_at DESC LIMIT ?1"))?;
            let rows = st.query_map([limit as i64], ws_row)?.collect::<rusqlite::Result<Vec<_>>>();
            rows
        })
    }

    pub fn update_workspace(&self, w: &WorkspaceRow) -> Result<()> {
        let s = serde_json::to_string(&w.settings)?;
        self.with(|c| {
            c.execute(
                "UPDATE workspaces SET display_name=?2, is_git=?3, default_mode=?4, library_profile_id=?5,
                 local_only=?6, pinned=?7, settings_json=?8 WHERE id=?1",
                params![w.id, w.display_name, w.is_git, w.default_mode, w.library_profile_id, w.local_only, w.pinned, s],
            )
        })?;
        Ok(())
    }

    pub fn remove_workspace(&self, id: &str) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM workspaces WHERE id = ?1", [id]))?;
        Ok(())
    }
}

// ---------- runs & steps ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RunRow {
    pub id: String,
    pub workspace_id: String,
    pub goal: String,
    pub mode: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub est_cost_usd: f64,
    pub peak_mem_mb: f64,
    pub library_snapshot_hash: Option<String>,
    pub budget_planning: bool,
    pub branch: Option<String>,
    pub base_ref: Option<String>,
    #[serde(serialize_with = "ser_obj", default)]
    pub summary: serde_json::Value,
}

fn run_row(r: &Row) -> rusqlite::Result<RunRow> {
    let s: String = r.get(13)?;
    Ok(RunRow {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        goal: r.get(2)?,
        mode: r.get(3)?,
        status: r.get(4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
        est_cost_usd: r.get(7)?,
        peak_mem_mb: r.get(8)?,
        library_snapshot_hash: r.get(9)?,
        budget_planning: r.get(10)?,
        branch: r.get(11)?,
        base_ref: r.get(12)?,
        summary: obj(&s),
    })
}

const RUN_COLS: &str = "id, workspace_id, goal, mode, status, started_at, ended_at, est_cost_usd, peak_mem_mb,
    library_snapshot_hash, budget_planning, branch, base_ref, summary_json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StepRow {
    pub id: String,
    pub run_id: String,
    pub idx: i64,
    pub title: String,
    pub class: String,
    pub kind: String,
    pub agent_id: Option<String>,
    pub model_id: Option<String>,
    pub tier: Option<String>,
    pub library_agent_id: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub escalated: bool,
    pub route_reason: Option<String>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_estimated: bool,
    pub cost_usd: f64,
    pub paid_equiv_usd: f64,
    pub commit_ref: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    #[serde(serialize_with = "ser_obj", default)]
    pub detail: serde_json::Value,
}

fn step_row(r: &Row) -> rusqlite::Result<StepRow> {
    let d: String = r.get(22)?;
    Ok(StepRow {
        id: r.get(0)?,
        run_id: r.get(1)?,
        idx: r.get(2)?,
        title: r.get(3)?,
        class: r.get(4)?,
        kind: r.get(5)?,
        agent_id: r.get(6)?,
        model_id: r.get(7)?,
        tier: r.get(8)?,
        library_agent_id: r.get(9)?,
        status: r.get(10)?,
        attempts: r.get(11)?,
        escalated: r.get(12)?,
        route_reason: r.get(13)?,
        tokens_in: r.get(14)?,
        tokens_out: r.get(15)?,
        tokens_estimated: r.get(16)?,
        cost_usd: r.get(17)?,
        paid_equiv_usd: r.get(18)?,
        commit_ref: r.get(19)?,
        started_at: r.get(20)?,
        ended_at: r.get(21)?,
        detail: obj(&d),
    })
}

const STEP_COLS: &str = "id, run_id, idx, title, class, kind, agent_id, model_id, tier, library_agent_id, status,
    attempts, escalated, route_reason, tokens_in, tokens_out, tokens_estimated, cost_usd, paid_equiv_usd,
    commit_ref, started_at, ended_at, detail_json";

impl Db {
    pub fn insert_run(&self, r: &RunRow) -> Result<()> {
        self.save_run(r)
    }

    pub fn save_run(&self, r: &RunRow) -> Result<()> {
        let s = if r.summary.is_object() { serde_json::to_string(&r.summary)? } else { "{}".to_string() };
        self.with(|c| {
            c.execute(
                &format!("INSERT OR REPLACE INTO runs ({RUN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"),
                params![
                    r.id,
                    r.workspace_id,
                    r.goal,
                    r.mode,
                    r.status,
                    r.started_at,
                    r.ended_at,
                    r.est_cost_usd,
                    r.peak_mem_mb,
                    r.library_snapshot_hash,
                    r.budget_planning,
                    r.branch,
                    r.base_ref,
                    s
                ],
            )
        })?;
        Ok(())
    }

    pub fn run(&self, id: &str) -> Result<Option<RunRow>> {
        self.with(|c| c.query_row(&format!("SELECT {RUN_COLS} FROM runs WHERE id = ?1"), [id], run_row).optional())
    }

    pub fn runs(&self, workspace_id: Option<&str>, since: Option<i64>, limit: usize) -> Result<Vec<RunRow>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {RUN_COLS} FROM runs WHERE (?1 IS NULL OR workspace_id = ?1) AND (?2 IS NULL OR started_at >= ?2)
                 ORDER BY started_at DESC LIMIT ?3"
            ))?;
            let rows = st.query_map(params![workspace_id, since, limit as i64], run_row)?.collect();
            rows
        })
    }

    /// Runs left non-terminal by a crash or force-quit are marked failed on start.
    pub fn fail_orphan_runs(&self) -> Result<usize> {
        let n = self.with(|c| {
            c.execute("UPDATE runs SET status='failed', ended_at=?1 WHERE status NOT IN ('succeeded','failed','cancelled')", [now_ms()])
        })?;
        self.with(|c| {
            c.execute(
                "UPDATE steps SET status='cancelled' WHERE status IN ('running','verifying','awaiting_approval','pending')
                 AND run_id IN (SELECT id FROM runs WHERE status='failed')",
                [],
            )
        })?;
        Ok(n)
    }

    pub fn save_step(&self, s: &StepRow) -> Result<()> {
        let d = if s.detail.is_object() { serde_json::to_string(&s.detail)? } else { "{}".to_string() };
        self.with(|c| {
            c.execute(
                &format!(
                    "INSERT OR REPLACE INTO steps ({STEP_COLS}) VALUES
                     (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)"
                ),
                params![
                    s.id,
                    s.run_id,
                    s.idx,
                    s.title,
                    s.class,
                    s.kind,
                    s.agent_id,
                    s.model_id,
                    s.tier,
                    s.library_agent_id,
                    s.status,
                    s.attempts,
                    s.escalated,
                    s.route_reason,
                    s.tokens_in,
                    s.tokens_out,
                    s.tokens_estimated,
                    s.cost_usd,
                    s.paid_equiv_usd,
                    s.commit_ref,
                    s.started_at,
                    s.ended_at,
                    d
                ],
            )
        })?;
        Ok(())
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRow>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {STEP_COLS} FROM steps WHERE run_id = ?1 ORDER BY idx"))?;
            let rows = st.query_map([run_id], step_row)?.collect();
            rows
        })
    }

    pub fn events_for_step(&self, step_id: &str, limit: usize) -> Result<Vec<EventRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, run_id, step_id, ts, type, payload_json FROM events WHERE step_id = ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows: Vec<EventRow> = st
                .query_map(params![step_id, limit as i64], |r| {
                    let p: String = r.get(5)?;
                    Ok(EventRow {
                        id: r.get(0)?,
                        run_id: r.get(1)?,
                        step_id: r.get(2)?,
                        ts: r.get(3)?,
                        kind: r.get(4)?,
                        payload: serde_json::from_str(&p).unwrap_or_default(),
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            rows.reverse();
            Ok(rows)
        })
    }

    /// Pass rate per (tier, model) across finished steps, for the router's
    /// suggestions and the "budget planning" comparison.
    pub fn outcome_stats(&self, workspace_id: Option<&str>) -> Result<Vec<OutcomeStat>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT s.class, s.tier, COALESCE(s.model_id, s.agent_id, ''), r.budget_planning,
                        SUM(CASE WHEN s.status IN ('passed','accepted') AND s.attempts <= 1 THEN 1 ELSE 0 END),
                        COUNT(*)
                 FROM steps s JOIN runs r ON r.id = s.run_id
                 WHERE s.kind = 'execute' AND s.status IN ('passed','accepted','failed')
                   AND (?1 IS NULL OR r.workspace_id = ?1)
                 GROUP BY 1, 2, 3, 4",
            )?;
            let rows = st
                .query_map([workspace_id], |r| {
                    Ok(OutcomeStat {
                        class: r.get(0)?,
                        tier: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        model: r.get(2)?,
                        budget_planning: r.get(3)?,
                        first_try_passes: r.get(4)?,
                        total: r.get(5)?,
                    })
                })?
                .collect();
            rows
        })
    }

    pub fn prune_metrics(&self, older_than_days: i64) -> Result<usize> {
        let cutoff = now_ms() - older_than_days * 86_400_000;
        let n = self.with(|c| c.execute("DELETE FROM metrics WHERE ts < ?1", [cutoff]))?;
        Ok(n)
    }

    pub fn metrics_since(&self, since: i64) -> Result<Vec<MetricRow>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT ts, target, rss_mb, cpu_pct, vram_mb FROM metrics WHERE ts >= ?1 ORDER BY ts")?;
            let rows = st
                .query_map([since], |r| {
                    Ok(MetricRow { ts: r.get(0)?, target: r.get(1)?, rss_mb: r.get(2)?, cpu_pct: r.get(3)?, vram_mb: r.get(4)? })
                })?
                .collect();
            rows
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EventRow {
    pub id: i64,
    pub run_id: String,
    pub step_id: Option<String>,
    pub ts: i64,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MetricRow {
    pub ts: i64,
    pub target: String,
    pub rss_mb: f64,
    pub cpu_pct: f64,
    pub vram_mb: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OutcomeStat {
    pub class: String,
    pub tier: String,
    pub model: String,
    pub budget_planning: bool,
    pub first_try_passes: i64,
    pub total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_defaults() {
        let db = Db::open_memory().unwrap();
        let mut s = db.settings();
        assert_eq!(s.default_mode, "balanced");
        assert_eq!(s.modes.len(), 3);
        s.approve_before_paid = true;
        s.modes[0].display_name = "Thrifty".into();
        db.save_settings(&s).unwrap();
        let s2 = db.settings();
        assert!(s2.approve_before_paid);
        assert_eq!(s2.mode("cost").display_name, "Thrifty");
    }

    #[test]
    fn workspace_upsert_is_unique_by_path() {
        let db = Db::open_memory().unwrap();
        let a = db.upsert_workspace("/tmp/x", "x", false).unwrap();
        let b = db.upsert_workspace("/tmp/x", "x", true).unwrap();
        assert_eq!(a.id, b.id);
        assert!(b.is_git);
        assert_eq!(db.recent_workspaces(10).unwrap().len(), 1);
    }

    #[test]
    fn runs_and_steps_roundtrip() {
        let db = Db::open_memory().unwrap();
        let run = RunRow {
            id: "r".into(),
            workspace_id: "w".into(),
            goal: "g".into(),
            mode: "balanced".into(),
            status: "running".into(),
            started_at: 1,
            ..Default::default()
        };
        db.insert_run(&run).unwrap();
        let step = StepRow {
            id: "s".into(),
            run_id: "r".into(),
            idx: 1,
            title: "t".into(),
            class: "low".into(),
            kind: "execute".into(),
            status: "pending".into(),
            ..Default::default()
        };
        db.save_step(&step).unwrap();
        assert_eq!(db.steps("r").unwrap().len(), 1);
        // Default (null) JSON fields come back and serialize as objects.
        assert!(db.run("r").unwrap().unwrap().summary.is_object());
        assert_eq!(serde_json::to_value(&step).unwrap()["detail"], serde_json::json!({}));
        assert_eq!(serde_json::to_value(&run).unwrap()["summary"], serde_json::json!({}));
        assert_eq!(db.fail_orphan_runs().unwrap(), 1);
        assert_eq!(db.steps("r").unwrap()[0].status, "cancelled");
    }
}

// ---------- agents, providers, models ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentRow {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub cli: String,
    pub version: Option<String>,
    pub install_path: Option<String>,
    pub auth_ok: bool,
    pub enabled: bool,
    pub supports_local_models: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProviderRow {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub base_url: Option<String>,
    pub key_ref: Option<String>,
    pub enabled: bool,
    pub monthly_cap_usd: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ModelRow {
    pub id: String,
    pub display_name: String,
    pub provider_id: String,
    pub runtime_id: Option<String>,
    pub name: String,
    pub tier: String,
    pub size_gb: Option<f64>,
    pub mem_needed_gb: Option<f64>,
    pub quant: Option<String>,
    pub ctx_len: Option<i64>,
    pub tool_calling: Option<bool>,
    pub price_in_per_m: Option<f64>,
    pub price_out_per_m: Option<f64>,
    pub path: Option<String>,
    pub installed: bool,
}

impl Db {
    /// Records detection results, keeping the user's display name and enabled flag.
    pub fn upsert_agent(&self, a: &AgentRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO agents (id, display_name, kind, cli, version, install_path, auth_ok, enabled, supports_local_models)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, cli=excluded.cli, version=excluded.version,
                   install_path=excluded.install_path, auth_ok=excluded.auth_ok, supports_local_models=excluded.supports_local_models",
                params![a.id, a.display_name, a.kind, a.cli, a.version, a.install_path, a.auth_ok, a.enabled, a.supports_local_models],
            )
        })?;
        Ok(())
    }

    pub fn agents(&self) -> Result<Vec<AgentRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, display_name, kind, cli, version, install_path, auth_ok, enabled, supports_local_models FROM agents",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(AgentRow {
                        id: r.get(0)?,
                        display_name: r.get(1)?,
                        kind: r.get(2)?,
                        cli: r.get(3)?,
                        version: r.get(4)?,
                        install_path: r.get(5)?,
                        auth_ok: r.get(6)?,
                        enabled: r.get(7)?,
                        supports_local_models: r.get(8)?,
                    })
                })?
                .collect();
            rows
        })
    }

    pub fn set_agent_meta(&self, id: &str, display_name: Option<&str>, enabled: Option<bool>) -> Result<()> {
        self.with(|c| {
            if let Some(n) = display_name {
                c.execute("UPDATE agents SET display_name=?2 WHERE id=?1", params![id, n])?;
            }
            if let Some(e) = enabled {
                c.execute("UPDATE agents SET enabled=?2 WHERE id=?1", params![id, e])?;
            }
            Ok(())
        })
    }

    pub fn save_provider(&self, p: &ProviderRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO providers (id, display_name, type, base_url, key_ref, enabled, monthly_cap_usd)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![p.id, p.display_name, p.kind, p.base_url, p.key_ref, p.enabled, p.monthly_cap_usd],
            )
        })?;
        Ok(())
    }

    pub fn providers(&self) -> Result<Vec<ProviderRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, display_name, type, base_url, key_ref, enabled, monthly_cap_usd FROM providers ORDER BY display_name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(ProviderRow {
                        id: r.get(0)?,
                        display_name: r.get(1)?,
                        kind: r.get(2)?,
                        base_url: r.get(3)?,
                        key_ref: r.get(4)?,
                        enabled: r.get(5)?,
                        monthly_cap_usd: r.get(6)?,
                    })
                })?
                .collect();
            rows
        })
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM providers WHERE id=?1", [id])?;
            c.execute("DELETE FROM models WHERE provider_id=?1", [id])
        })?;
        Ok(())
    }

    pub fn save_model(&self, m: &ModelRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO models (id, display_name, provider_id, runtime_id, name, tier, size_gb, mem_needed_gb,
                 quant, ctx_len, tool_calling, price_in_per_m, price_out_per_m, path, installed)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    m.id,
                    m.display_name,
                    m.provider_id,
                    m.runtime_id,
                    m.name,
                    m.tier,
                    m.size_gb,
                    m.mem_needed_gb,
                    m.quant,
                    m.ctx_len,
                    m.tool_calling,
                    m.price_in_per_m,
                    m.price_out_per_m,
                    m.path,
                    m.installed
                ],
            )
        })?;
        Ok(())
    }

    pub fn models(&self) -> Result<Vec<ModelRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, display_name, provider_id, runtime_id, name, tier, size_gb, mem_needed_gb, quant, ctx_len,
                 tool_calling, price_in_per_m, price_out_per_m, path, installed FROM models ORDER BY tier, display_name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(ModelRow {
                        id: r.get(0)?,
                        display_name: r.get(1)?,
                        provider_id: r.get(2)?,
                        runtime_id: r.get(3)?,
                        name: r.get(4)?,
                        tier: r.get(5)?,
                        size_gb: r.get(6)?,
                        mem_needed_gb: r.get(7)?,
                        quant: r.get(8)?,
                        ctx_len: r.get(9)?,
                        tool_calling: r.get(10)?,
                        price_in_per_m: r.get(11)?,
                        price_out_per_m: r.get(12)?,
                        path: r.get(13)?,
                        installed: r.get(14)?,
                    })
                })?
                .collect();
            rows
        })
    }

    pub fn delete_model(&self, id: &str) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM models WHERE id=?1", [id]))?;
        Ok(())
    }

    /// Estimated spend on a provider since `since` (for monthly caps).
    pub fn provider_spend_since(&self, provider_id: &str, since: i64) -> Result<f64> {
        let like = format!("{provider_id}/%");
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(SUM(cost_usd), 0) FROM steps WHERE model_id LIKE ?1 AND COALESCE(started_at, 0) >= ?2",
                params![like, since],
                |r| r.get(0),
            )
        })
    }

    pub fn profiles(&self) -> Result<Vec<(String, String, Vec<String>)>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, display_name, item_ids_json FROM library_profiles ORDER BY display_name")?;
            let rows = st
                .query_map([], |r| {
                    let j: String = r.get(2)?;
                    Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&j).unwrap_or_default()))
                })?
                .collect();
            rows
        })
    }

    pub fn save_profile(&self, id: &str, name: &str, items: &[String]) -> Result<()> {
        let j = serde_json::to_string(items)?;
        self.with(|c| {
            c.execute("INSERT OR REPLACE INTO library_profiles (id, display_name, item_ids_json) VALUES (?1,?2,?3)", params![id, name, j])
        })?;
        Ok(())
    }

    pub fn delete_profile(&self, id: &str) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM library_profiles WHERE id=?1", [id]))?;
        Ok(())
    }
}
