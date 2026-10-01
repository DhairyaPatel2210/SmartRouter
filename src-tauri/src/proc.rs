//! Child-process plumbing shared by adapters, checks and installers:
//! PATH resolution for GUI launches, lowered priority, process groups so a
//! cancel kills the whole tree, line streaming and timeouts.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

/// Nice value for agent CLIs, checks and the app-managed model server.
pub const LOW_PRIORITY_NICE: i32 = 10;

static PATH_ENV: OnceLock<String> = OnceLock::new();

/// PATH for spawned tools. Apps launched from Finder get a minimal PATH, so
/// we ask the user's login shell once (off the UI thread) and add the usual
/// install locations.
pub fn path_env() -> &'static str {
    PATH_ENV.get_or_init(|| {
        let mut parts: Vec<String> = Vec::new();
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            if let Ok(out) = std::process::Command::new(&shell)
                .args(["-ilc", "printf '%s' \"$PATH\""])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
            {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    // Interactive shells may print banners; PATH is the last line.
                    if let Some(line) = s.lines().last() {
                        parts.extend(line.split(':').filter(|p| !p.is_empty()).map(String::from));
                    }
                }
            }
        }
        if let Ok(p) = std::env::var("PATH") {
            parts.extend(p.split(':').map(String::from));
        }
        if let Some(home) = dirs::home_dir() {
            for sub in [".local/bin", ".cargo/bin", ".bun/bin", ".npm-global/bin", ".opencode/bin", ".volta/bin", "bin"] {
                parts.push(home.join(sub).to_string_lossy().into_owned());
            }
        }
        for p in ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
            parts.push(p.into());
        }
        let mut seen = std::collections::HashSet::new();
        parts.retain(|p| seen.insert(p.clone()));
        parts.join(if cfg!(windows) { ";" } else { ":" })
    })
}

/// Finds an executable on the resolved PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    if bin.contains('/') {
        let p = PathBuf::from(bin);
        return p.is_file().then_some(p);
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path_env().split(sep) {
        let p = Path::new(dir).join(bin);
        if is_executable(&p) {
            return Some(p);
        }
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat"] {
            let pe = p.with_extension(ext);
            if pe.is_file() {
                return Some(pe);
            }
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Builds a command with PATH set, stdin closed, its own process group and
/// (optionally) lowered priority.
pub fn command(program: impl AsRef<std::ffi::OsStr>, low_priority: bool) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PATH", path_env()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
        if low_priority {
            // SAFETY: setpriority is async-signal-safe; runs in the child before exec.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setpriority(libc::PRIO_PROCESS, 0, LOW_PRIORITY_NICE);
                    Ok(())
                });
            }
        }
    }
    #[cfg(not(unix))]
    let _ = low_priority;
    cmd
}

/// Kills a process group: SIGTERM, then SIGKILL after a grace period.
pub async fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        let pgid = pid as i32;
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if unsafe { libc::killpg(pgid, 0) } != 0 {
                return;
            }
        }
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output();
    }
}

/// Every live child the app started, keyed by an owner label (step id,
/// "install:<id>", "ollama"…). Used by Stop everything and the metrics sampler.
#[derive(Clone, Default)]
pub struct ProcRegistry {
    inner: Arc<Mutex<HashMap<u32, ProcInfo>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub owner: String,
    /// Human label, e.g. "OpenCode · step 3".
    pub label: String,
}

impl ProcRegistry {
    pub fn add(&self, pid: u32, owner: &str, label: &str) {
        self.inner.lock().insert(pid, ProcInfo { pid, owner: owner.into(), label: label.into() });
    }
    pub fn remove(&self, pid: u32) {
        self.inner.lock().remove(&pid);
    }
    pub fn list(&self) -> Vec<ProcInfo> {
        self.inner.lock().values().cloned().collect()
    }
    pub fn by_owner(&self, owner: &str) -> Vec<u32> {
        self.inner.lock().values().filter(|p| p.owner == owner).map(|p| p.pid).collect()
    }
    pub async fn kill_owner(&self, owner: &str) {
        for pid in self.by_owner(owner) {
            kill_tree(pid).await;
            self.remove(pid);
        }
    }
    pub async fn kill_all(&self, except_owner: Option<&str>) {
        let pids: Vec<u32> = self.inner.lock().values().filter(|p| Some(p.owner.as_str()) != except_owner).map(|p| p.pid).collect();
        for pid in pids {
            kill_tree(pid).await;
            self.remove(pid);
        }
    }
}

/// Cooperative cancellation shared between the engine and a running child.
#[derive(Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl Default for CancelToken {
    fn default() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { flag: Arc::new(AtomicBool::new(false)), tx: Arc::new(tx), rx }
    }
}

impl CancelToken {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _ = self.tx.send(true);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
    /// A child token cancelled when either it or its parent is.
    pub fn child(&self) -> CancelToken {
        let child = CancelToken::default();
        let parent = self.clone();
        let c = child.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = parent.cancelled() => c.cancel(),
                _ = c.cancelled() => {}
            }
        });
        child
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub enum ExitKind {
    Exited(i32),
    Signaled,
    TimedOut,
    Cancelled,
}

impl ExitKind {
    pub fn success(&self) -> bool {
        matches!(self, ExitKind::Exited(0))
    }
}

pub struct RunOpts<'a> {
    pub registry: &'a ProcRegistry,
    pub owner: &'a str,
    pub label: &'a str,
    pub timeout: Duration,
    pub cancel: &'a CancelToken,
}

/// Spawns `cmd`, calls `on_line` for every output line as it arrives (no
/// whole-output buffering) and enforces the timeout and cancellation by
/// killing the whole process group.
pub async fn run_streaming(mut cmd: Command, opts: RunOpts<'_>, mut on_line: impl FnMut(Stream, &str)) -> Result<ExitKind> {
    let mut child: Child = cmd.spawn().context("failed to start process")?;
    let pid = child.id().ok_or_else(|| anyhow!("process exited immediately"))?;
    opts.registry.add(pid, opts.owner, opts.label);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (line_tx, mut line_rx) = tokio::sync::mpsc::channel::<(Stream, String)>(512);
    let mut readers = Vec::new();
    if let Some(out) = stdout {
        let tx = line_tx.clone();
        readers.push(tokio::spawn(async move {
            let mut r = BufReader::with_capacity(16 * 1024, out).lines();
            while let Ok(Some(l)) = r.next_line().await {
                if tx.send((Stream::Stdout, l)).await.is_err() {
                    break;
                }
            }
        }));
    }
    if let Some(err) = stderr {
        let tx = line_tx.clone();
        readers.push(tokio::spawn(async move {
            let mut r = BufReader::with_capacity(8 * 1024, err).lines();
            while let Ok(Some(l)) = r.next_line().await {
                if tx.send((Stream::Stderr, l)).await.is_err() {
                    break;
                }
            }
        }));
    }
    drop(line_tx);

    let deadline = tokio::time::sleep(opts.timeout);
    tokio::pin!(deadline);
    let mut outcome: Option<ExitKind> = None;
    let mut lines_open = true;
    loop {
        tokio::select! {
            maybe = line_rx.recv(), if lines_open => match maybe {
                Some((s, l)) => on_line(s, &l),
                None => lines_open = false,
            },
            status = child.wait(), if outcome.is_none() => {
                outcome = Some(match status {
                    Ok(st) => st.code().map(ExitKind::Exited).unwrap_or(ExitKind::Signaled),
                    Err(_) => ExitKind::Signaled,
                });
            }
            _ = &mut deadline, if outcome.is_none() => {
                kill_tree(pid).await;
                outcome = Some(ExitKind::TimedOut);
            }
            _ = opts.cancel.cancelled(), if outcome.is_none() => {
                kill_tree(pid).await;
                outcome = Some(ExitKind::Cancelled);
            }
        }
        if outcome.is_some() && !lines_open {
            break;
        }
        // Grandchildren can keep pipes open after the main process exits;
        // don't wait on them forever.
        if outcome.is_some() {
            match tokio::time::timeout(Duration::from_millis(500), line_rx.recv()).await {
                Ok(Some((s, l))) => on_line(s, &l),
                _ => break,
            }
        }
    }
    for r in readers {
        r.abort();
    }
    opts.registry.remove(pid);
    Ok(outcome.unwrap_or(ExitKind::Signaled))
}

/// Runs a short command and returns trimmed stdout (for detection/version checks).
pub async fn output(program: &str, args: &[&str], cwd: Option<&Path>, timeout: Duration) -> Result<(i32, String, String)> {
    let mut cmd = command(program, false);
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let fut = cmd.output();
    let out = tokio::time::timeout(timeout, fut).await.map_err(|_| anyhow!("{program} timed out"))??;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_lines_and_exit_code() {
        let reg = ProcRegistry::default();
        let cancel = CancelToken::default();
        let mut cmd = command("sh", true);
        cmd.args(["-c", "echo one; echo two 1>&2; exit 3"]);
        let mut lines = vec![];
        let exit = run_streaming(
            cmd,
            RunOpts { registry: &reg, owner: "t", label: "t", timeout: Duration::from_secs(5), cancel: &cancel },
            |s, l| lines.push((s, l.to_string())),
        )
        .await
        .unwrap();
        assert!(matches!(exit, ExitKind::Exited(3)));
        assert!(lines.contains(&(Stream::Stdout, "one".into())));
        assert!(lines.contains(&(Stream::Stderr, "two".into())));
        assert!(reg.list().is_empty());
    }

    #[tokio::test]
    async fn timeout_kills_whole_tree() {
        let reg = ProcRegistry::default();
        let cancel = CancelToken::default();
        let mut cmd = command("sh", false);
        // The grandchild sleep must die with the group.
        cmd.args(["-c", "sleep 30 & sleep 30; wait"]);
        let t = std::time::Instant::now();
        let exit = run_streaming(
            cmd,
            RunOpts { registry: &reg, owner: "t", label: "t", timeout: Duration::from_millis(300), cancel: &cancel },
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(matches!(exit, ExitKind::TimedOut));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn cancel_token_stops_process() {
        let reg = ProcRegistry::default();
        let cancel = CancelToken::default();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            c2.cancel();
        });
        let mut cmd = command("sleep", false);
        cmd.arg("30");
        let exit = run_streaming(
            cmd,
            RunOpts { registry: &reg, owner: "t", label: "t", timeout: Duration::from_secs(30), cancel: &cancel },
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(matches!(exit, ExitKind::Cancelled));
    }

    #[test]
    fn which_finds_sh() {
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
    }
}
