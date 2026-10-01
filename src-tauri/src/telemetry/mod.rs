//! Local observability: the batched UI event bus, per-step log ring buffers
//! (full logs stream to disk), the resource sampler and cost estimates.
//! Nothing here sends data anywhere.

pub mod cost;
pub mod sampler;

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// The UI receives events in batches at most every 50 ms.
pub const UI_BATCH: Duration = Duration::from_millis(50);

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Run {
        run: crate::db::queries::RunRow,
    },
    Step {
        step: crate::db::queries::StepRow,
    },
    Log {
        run_id: String,
        step_id: String,
        line: LogLine,
    },
    /// Governor/user-facing notice (banner + run log).
    Notice {
        level: String,
        text: String,
        run_id: Option<String>,
    },
    Approval {
        request: crate::engine::ApprovalRequest,
    },
    ApprovalDone {
        id: String,
    },
    Resources {
        snapshot: sampler::Snapshot,
    },
    Install {
        id: String,
        line: String,
        done: Option<bool>,
        progress: Option<f64>,
    },
    Pull {
        model: String,
        status: String,
        completed: u64,
        total: u64,
        done: Option<bool>,
        error: Option<String>,
    },
    Move {
        completed: u64,
        total: u64,
        done: Option<bool>,
        error: Option<String>,
    },
    Library {
        workspace: Option<String>,
    },
    Agents,
}

pub trait Emitter: Send + Sync {
    fn emit(&self, batch: Vec<UiEvent>);
}

/// Coalesces events and flushes them every [`UI_BATCH`]. The flush task
/// sleeps on a Notify while idle (no timer).
#[derive(Clone)]
pub struct Bus {
    buf: Arc<Mutex<Vec<UiEvent>>>,
    notify: Arc<Notify>,
}

impl Bus {
    pub fn new(emitter: Arc<dyn Emitter>) -> Self {
        let bus = Self { buf: Arc::new(Mutex::new(Vec::with_capacity(256))), notify: Arc::new(Notify::new()) };
        let b = bus.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                b.notify.notified().await;
                tokio::time::sleep(UI_BATCH).await;
                let batch = std::mem::take(&mut *b.buf.lock());
                if !batch.is_empty() {
                    emitter.emit(batch);
                }
            }
        });
        bus
    }

    pub fn send(&self, e: UiEvent) {
        let first = {
            let mut b = self.buf.lock();
            b.push(e);
            b.len() == 1
        };
        if first {
            self.notify.notify_one();
        }
    }

    pub fn notice(&self, level: &str, text: impl Into<String>, run_id: Option<&str>) {
        self.send(UiEvent::Notice { level: level.into(), text: text.into(), run_id: run_id.map(String::from) });
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct LogLine {
    pub seq: u64,
    pub ts: i64,
    /// out | tool | edit | error | governor | check | route | info
    pub kind: String,
    pub text: String,
}

struct StepLog {
    ring: VecDeque<LogLine>,
    file: Option<std::io::BufWriter<std::fs::File>>,
    seq: u64,
}

/// Bounded in-memory log per step; the full log streams to
/// `<data>/logs/<run-id>/<step-idx>.log` through a buffered writer.
#[derive(Clone)]
pub struct Logs {
    inner: Arc<Mutex<HashMap<String, StepLog>>>,
    dir: PathBuf,
    cap: Arc<Mutex<usize>>,
}

impl Logs {
    pub fn new(data_dir: &std::path::Path, cap: usize) -> Self {
        Self { inner: Default::default(), dir: data_dir.join("logs"), cap: Arc::new(Mutex::new(cap.max(100))) }
    }

    pub fn set_cap(&self, cap: usize) {
        *self.cap.lock() = cap.max(100);
    }

    pub fn path(&self, run_id: &str, step_idx: i64) -> PathBuf {
        self.dir.join(run_id).join(format!("{step_idx}.log"))
    }

    pub fn open(&self, run_id: &str, step_id: &str, step_idx: i64) {
        let p = self.path(run_id, step_idx);
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let file =
            std::fs::OpenOptions::new().create(true).append(true).open(p).ok().map(|f| std::io::BufWriter::with_capacity(32 * 1024, f));
        let mut g = self.inner.lock();
        let e = g.entry(step_id.to_string()).or_insert_with(|| StepLog { ring: VecDeque::new(), file: None, seq: 0 });
        e.file = file;
    }

    pub fn push(&self, step_id: &str, kind: &str, text: &str) -> LogLine {
        let cap = *self.cap.lock();
        let mut g = self.inner.lock();
        let e = g.entry(step_id.to_string()).or_insert_with(|| StepLog { ring: VecDeque::new(), file: None, seq: 0 });
        e.seq += 1;
        let line = LogLine { seq: e.seq, ts: crate::db::now_ms(), kind: kind.to_string(), text: text.to_string() };
        if let Some(f) = e.file.as_mut() {
            let _ = writeln!(f, "[{}] {}", kind, text);
        }
        if e.ring.len() >= cap {
            e.ring.pop_front();
        }
        e.ring.push_back(line.clone());
        line
    }

    /// Flushes and closes the step's log file (keeps the ring buffer).
    pub fn close(&self, step_id: &str) {
        if let Some(e) = self.inner.lock().get_mut(step_id) {
            if let Some(mut f) = e.file.take() {
                let _ = f.flush();
            }
        }
    }

    /// Lines after `after_seq` from memory, or the tail of the file on disk
    /// for steps from earlier sessions.
    pub fn get(&self, run_id: &str, step_id: &str, step_idx: i64, after_seq: u64) -> Vec<LogLine> {
        if let Some(e) = self.inner.lock().get(step_id) {
            return e.ring.iter().filter(|l| l.seq > after_seq).cloned().collect();
        }
        let cap = *self.cap.lock();
        let Ok(s) = std::fs::read_to_string(self.path(run_id, step_idx)) else { return vec![] };
        let lines: Vec<&str> = s.lines().collect();
        let start = lines.len().saturating_sub(cap);
        lines[start..]
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let (kind, text) = l.strip_prefix('[').and_then(|r| r.split_once("] ")).unwrap_or(("out", l));
                LogLine { seq: (start + i + 1) as u64, ts: 0, kind: kind.into(), text: text.into() }
            })
            .filter(|l| l.seq > after_seq)
            .collect()
    }

    /// Frees memory for a finished run's steps.
    pub fn drop_run(&self, step_ids: &[String]) {
        let mut g = self.inner.lock();
        for s in step_ids {
            g.remove(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_is_bounded_and_file_has_everything() {
        let d = tempfile::tempdir().unwrap();
        let logs = Logs::new(d.path(), 100);
        logs.open("r", "s", 1);
        for i in 0..250 {
            logs.push("s", "out", &format!("line {i}"));
        }
        logs.close("s");
        let mem = logs.get("r", "s", 1, 0);
        assert_eq!(mem.len(), 100);
        assert_eq!(mem.last().unwrap().text, "line 249");
        assert_eq!(logs.get("r", "s", 1, 240).len(), 10);
        let file = std::fs::read_to_string(logs.path("r", 1)).unwrap();
        assert_eq!(file.lines().count(), 250);
        // After a restart the tail comes from disk.
        logs.drop_run(&["s".into()]);
        let disk = logs.get("r", "s", 1, 0);
        assert_eq!(disk.len(), 100);
        assert_eq!(disk.last().unwrap().text, "line 249");
    }
}
