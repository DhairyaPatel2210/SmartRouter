//! Workspaces: the directory agents work in. Inspection (git state, write
//! access), the unsafe-root guard, the scope guard, git helpers and
//! snapshots for non-git folders.

pub mod git;
pub mod snapshot;

use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
pub struct WorkspaceInfo {
    pub path: String,
    pub name: String,
    pub exists: bool,
    pub is_git: bool,
    pub branch: Option<String>,
    pub dirty_files: Vec<String>,
    pub dirty_count: usize,
    pub writable: bool,
    /// Home, `/` or a system folder: needs an extra confirmation.
    pub unsafe_root: bool,
    pub detected_checks: Vec<String>,
    /// Existing agent config found here that can be imported into the Library.
    pub importable: Vec<String>,
}

pub fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub async fn inspect(path: &Path) -> WorkspaceInfo {
    let exists = path.is_dir();
    let is_git = exists && git::is_repo(path).await;
    let (branch, dirty) = if is_git {
        (git::current_branch(path).await, git::dirty_files(path).await.unwrap_or_default())
    } else {
        (None, vec![])
    };
    WorkspaceInfo {
        path: path.to_string_lossy().into_owned(),
        name: display_name(path),
        exists,
        is_git,
        branch,
        dirty_count: dirty.len(),
        dirty_files: dirty.into_iter().take(20).collect(),
        writable: exists && is_writable(path),
        unsafe_root: is_unsafe_root(path),
        detected_checks: if exists { crate::verify::detect_checks(path) } else { vec![] },
        importable: if exists { crate::library::import::candidates(path) } else { vec![] },
    }
}

fn is_writable(p: &Path) -> bool {
    let probe = p.join(format!(".{}-write-probe-{}", crate::brand::DATA_DIR_NAME, std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

/// The home directory, `/`, and system folders need an explicit extra confirmation.
pub fn is_unsafe_root(p: &Path) -> bool {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if p.parent().is_none() {
        return true;
    }
    if let Some(home) = dirs::home_dir() {
        let home = home.canonicalize().unwrap_or(home);
        if p == home {
            return true;
        }
        for sub in ["Library", "Desktop", "Documents", "Downloads", "Pictures", "Music", "Movies", ".ssh", ".config"] {
            if p == home.join(sub) {
                return true;
            }
        }
    }
    const SYSTEM: &[&str] = &[
        "/System", "/Library", "/usr", "/bin", "/sbin", "/etc", "/var", "/private", "/Applications", "/opt", "/Users", "/Volumes",
        "/dev", "/tmp", "/cores",
    ];
    let s = p.to_string_lossy();
    SYSTEM.iter().any(|root| s == *root || (s.starts_with(&format!("{root}/")) && is_system_subtree(root)))
}

fn is_system_subtree(root: &str) -> bool {
    // Anything under these is system territory; under /Users, /Volumes, /tmp,
    // /opt and /private only the root itself is unsafe (projects live there).
    matches!(root, "/System" | "/usr" | "/bin" | "/sbin" | "/etc" | "/dev" | "/cores")
}

/// Lexically normalizes `p` (resolving `.` and `..`) without touching disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Scope guard: is a path an agent reported (absolute or relative) inside the workspace?
pub fn is_inside(ws: &Path, reported: &str) -> bool {
    let p = Path::new(reported);
    let abs = if p.is_absolute() { p.to_path_buf() } else { ws.join(p) };
    let abs = normalize(&abs);
    let ws_c = ws.canonicalize().unwrap_or_else(|_| ws.to_path_buf());
    let ws_n = normalize(ws);
    // Compare both the literal and canonical forms (macOS /tmp → /private/tmp).
    if abs.starts_with(&ws_n) || abs.starts_with(&ws_c) {
        return true;
    }
    if let Some(parent) = abs.parent() {
        if let Ok(pc) = parent.canonicalize() {
            return pc.starts_with(&ws_c);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_roots() {
        assert!(is_unsafe_root(Path::new("/")));
        assert!(is_unsafe_root(&dirs::home_dir().unwrap()));
        assert!(is_unsafe_root(Path::new("/usr/local")));
        assert!(is_unsafe_root(Path::new("/System")));
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("proj");
        std::fs::create_dir(&proj).unwrap();
        assert!(!is_unsafe_root(&proj));
    }

    #[test]
    fn scope_guard() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path();
        assert!(is_inside(ws, "src/main.rs"));
        assert!(is_inside(ws, &ws.join("a/b.txt").to_string_lossy()));
        assert!(!is_inside(ws, "../outside.txt"));
        assert!(!is_inside(ws, "/etc/passwd"));
        assert!(!is_inside(ws, "src/../../escape"));
    }
}
