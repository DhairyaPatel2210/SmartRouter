//! SQLite storage. One connection for reads and rare writes (behind a mutex),
//! plus a background [`Writer`] that batches high-volume inserts (events,
//! metrics) so a streaming run never does one disk write per log line.

mod schema;
pub mod queries;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
pub const FLUSH_MAX_ROWS: usize = 200;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    writer: Writer,
    path: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = open_conn(path)?;
        migrate(&conn)?;
        let writer = Writer::spawn(open_conn(path)?);
        Ok(Self { conn: Arc::new(Mutex::new(conn)), writer, path: path.to_path_buf() })
    }

    /// In-memory database for tests. The writer shares a named in-memory DB.
    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let name = format!("file:mem{}?mode=memory&cache=shared", uuid::Uuid::new_v4().simple());
        let flags = rusqlite::OpenFlags::default() | rusqlite::OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(&name, flags)?;
        migrate(&conn)?;
        let wconn = Connection::open_with_flags(&name, flags)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)), writer: Writer::spawn(wconn), path: PathBuf::new() })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let c = self.conn.lock();
        f(&c).map_err(Into::into)
    }

    pub fn writer(&self) -> &Writer {
        &self.writer
    }

    /// Block until everything queued on the writer is on disk.
    pub fn flush(&self) {
        self.writer.flush();
    }
}

fn open_conn(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=3000;
         PRAGMA temp_store=MEMORY;",
    )?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in schema::MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql).with_context(|| format!("migration {}", i + 1))?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub enum WriteOp {
    Event { run_id: String, step_id: Option<String>, ts: i64, kind: String, payload: String },
    Metric { ts: i64, target: String, rss_mb: f64, cpu_pct: f64, vram_mb: Option<f64> },
    Flush(mpsc::SyncSender<()>),
}

/// Background batch writer: flushes every [`FLUSH_INTERVAL`] or
/// [`FLUSH_MAX_ROWS`] rows, whichever comes first. Idle = blocked on recv.
#[derive(Clone)]
pub struct Writer {
    tx: Sender<WriteOp>,
}

impl Writer {
    fn spawn(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<WriteOp>();
        std::thread::Builder::new()
            .name("db-writer".into())
            .spawn(move || {
                let mut buf: Vec<WriteOp> = Vec::with_capacity(FLUSH_MAX_ROWS);
                let mut first_at: Option<Instant> = None;
                loop {
                    let op = match first_at {
                        // Nothing pending: sleep until a row arrives (no timer at idle).
                        None => match rx.recv() {
                            Ok(op) => Some(op),
                            Err(_) => break,
                        },
                        Some(t) => {
                            let left = FLUSH_INTERVAL.saturating_sub(t.elapsed());
                            match rx.recv_timeout(left) {
                                Ok(op) => Some(op),
                                Err(RecvTimeoutError::Timeout) => None,
                                Err(RecvTimeoutError::Disconnected) => {
                                    write_batch(&conn, &mut buf);
                                    break;
                                }
                            }
                        }
                    };
                    match op {
                        Some(WriteOp::Flush(done)) => {
                            write_batch(&conn, &mut buf);
                            first_at = None;
                            let _ = done.send(());
                        }
                        Some(op) => {
                            buf.push(op);
                            first_at.get_or_insert_with(Instant::now);
                            if buf.len() >= FLUSH_MAX_ROWS {
                                write_batch(&conn, &mut buf);
                                first_at = None;
                            }
                        }
                        None => {
                            write_batch(&conn, &mut buf);
                            first_at = None;
                        }
                    }
                }
            })
            .expect("spawn db writer");
        Self { tx }
    }

    pub fn send(&self, op: WriteOp) {
        let _ = self.tx.send(op);
    }

    pub fn event(&self, run_id: &str, step_id: Option<&str>, kind: &str, payload: String) {
        self.send(WriteOp::Event {
            run_id: run_id.to_string(),
            step_id: step_id.map(str::to_string),
            ts: now_ms(),
            kind: kind.to_string(),
            payload,
        });
    }

    pub fn flush(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.send(WriteOp::Flush(tx));
        let _ = rx.recv_timeout(Duration::from_secs(5));
    }
}

fn write_batch(conn: &Connection, buf: &mut Vec<WriteOp>) {
    if buf.is_empty() {
        return;
    }
    let res: rusqlite::Result<()> = (|| {
        let tx = conn.unchecked_transaction()?;
        {
            let mut ev = tx.prepare_cached(
                "INSERT INTO events (run_id, step_id, ts, type, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut me = tx.prepare_cached(
                "INSERT INTO metrics (ts, target, rss_mb, cpu_pct, vram_mb) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for op in buf.iter() {
                match op {
                    WriteOp::Event { run_id, step_id, ts, kind, payload } => {
                        ev.execute(params![run_id, step_id, ts, kind, payload])?;
                    }
                    WriteOp::Metric { ts, target, rss_mb, cpu_pct, vram_mb } => {
                        me.execute(params![ts, target, rss_mb, cpu_pct, vram_mb])?;
                    }
                    WriteOp::Flush(_) => {}
                }
            }
        }
        tx.commit()
    })();
    if let Err(e) = res {
        log::error!("db batch write failed: {e}");
    }
    buf.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            migrate(c).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            assert_eq!(v as usize, schema::MIGRATIONS.len());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn writer_flushes_on_count_and_time() {
        let db = Db::open_memory().unwrap();
        for i in 0..(FLUSH_MAX_ROWS + 5) {
            db.writer().event("r1", Some("s1"), "stdout", format!("{{\"line\":{i}}}"));
        }
        // The first FLUSH_MAX_ROWS rows are written without waiting for the timer.
        std::thread::sleep(Duration::from_millis(50));
        let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))).unwrap();
        assert!(n >= FLUSH_MAX_ROWS as i64, "count flush, got {n}");
        // The remainder is written once the interval elapses.
        std::thread::sleep(FLUSH_INTERVAL + Duration::from_millis(150));
        let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))).unwrap();
        assert_eq!(n, (FLUSH_MAX_ROWS + 5) as i64);
    }

    #[test]
    fn explicit_flush_writes_everything() {
        let db = Db::open_memory().unwrap();
        db.writer().event("r1", None, "governor", "{}".into());
        db.flush();
        let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))).unwrap();
        assert_eq!(n, 1);
    }
}
