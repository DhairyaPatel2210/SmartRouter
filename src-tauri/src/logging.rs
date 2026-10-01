//! App log: stderr (visible in `npm run app:dev`) and `<data>/logs/app.log`
//! (rotated at 5 MB). Level from `ORCH_LOG` (error|warn|info|debug|trace);
//! default `info`, or `debug` in debug builds. Agent output lines are logged
//! at debug level so the default log stays readable.

use log::{Level, LevelFilter, Metadata, Record};
use parking_lot::Mutex;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 5 * 1024 * 1024;

struct Logger {
    file: Mutex<Option<std::fs::File>>,
    path: PathBuf,
    level: LevelFilter,
}

impl log::Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.level && (m.target().starts_with("orchestrator") || m.level() <= Level::Warn)
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let now = chrono_like_now();
        let target = r.target().trim_start_matches("orchestrator_lib::").trim_start_matches("orchestrator_lib");
        let line = format!("{now} {:<5} {:<18} {}\n", r.level(), target, r.args());
        let _ = std::io::stderr().write_all(line.as_bytes());
        let mut f = self.file.lock();
        if let Some(file) = f.as_mut() {
            let _ = file.write_all(line.as_bytes());
            if file.metadata().map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
                let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
                *f = open(&self.path);
            }
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

fn open(p: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
}

/// "HH:MM:SS.mmm" in local time.
fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    format!("{:02}:{:02}:{:02}.{:03}", tm.tm_hour, tm.tm_min, tm.tm_sec, now.subsec_millis())
}

/// Installs the logger once. Returns the log file path.
pub fn init(data_dir: &Path) -> PathBuf {
    let dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("app.log");
    let level = match std::env::var("ORCH_LOG").ok().as_deref() {
        Some("error") => LevelFilter::Error,
        Some("warn") => LevelFilter::Warn,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        Some("info") => LevelFilter::Info,
        _ if cfg!(debug_assertions) => LevelFilter::Debug,
        _ => LevelFilter::Info,
    };
    let logger = Logger { file: Mutex::new(open(&path)), path: path.clone(), level };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
    path
}
