//! Per-step snapshots for workspaces without git, so rollback still works.
//! Content-addressed: every file version is stored once under
//! `<data>/snapshots/<run-id>/objects/<sha256>`, and each step keeps a
//! manifest (path → hash). Restoring step k rewrites files to manifest k.

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Heavy or generated folders we never snapshot.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".gradle",
    ".idea",
    ".cache",
    "Pods",
    ".turbo",
    ".svelte-kit",
    "coverage",
];
const MAX_FILE: u64 = 20 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;

pub type Manifest = BTreeMap<String, String>;

pub struct SnapshotStore {
    root: PathBuf,
}

impl SnapshotStore {
    pub fn new(data_dir: &Path, run_id: &str) -> Self {
        Self { root: data_dir.join("snapshots").join(run_id) }
    }

    fn obj(&self, hash: &str) -> PathBuf {
        self.root.join("objects").join(&hash[..2]).join(hash)
    }

    fn manifest_path(&self, label: &str) -> PathBuf {
        self.root.join("manifests").join(format!("{label}.json"))
    }

    /// Records the current state of the workspace under `label` (e.g. "base", "step-3").
    pub fn take(&self, ws: &Path, label: &str) -> Result<Manifest> {
        let mut m = Manifest::new();
        let mut total = 0u64;
        walk(ws, ws, &mut |rel, abs| {
            let meta = std::fs::metadata(abs)?;
            if meta.len() > MAX_FILE {
                return Ok(());
            }
            total += meta.len();
            if total > MAX_TOTAL {
                bail!("workspace is too large to snapshot without git (over 1 GB); initialise git instead");
            }
            let bytes = std::fs::read(abs)?;
            let hash = hex::encode(Sha256::digest(&bytes));
            let o = self.obj(&hash);
            if !o.exists() {
                std::fs::create_dir_all(o.parent().unwrap())?;
                std::fs::write(&o, &bytes)?;
            }
            m.insert(rel.to_string(), hash);
            Ok(())
        })?;
        let mp = self.manifest_path(label);
        std::fs::create_dir_all(mp.parent().unwrap())?;
        std::fs::write(mp, serde_json::to_vec(&m)?)?;
        Ok(m)
    }

    pub fn load(&self, label: &str) -> Result<Manifest> {
        Ok(serde_json::from_slice(&std::fs::read(self.manifest_path(label))?)?)
    }

    /// Files that differ between two manifests.
    pub fn changed(a: &Manifest, b: &Manifest) -> Vec<String> {
        let mut out: Vec<String> = b.iter().filter(|(k, v)| a.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
        out.extend(a.keys().filter(|k| !b.contains_key(*k)).cloned());
        out.sort();
        out
    }

    /// Restores the workspace to the state recorded under `label`.
    pub fn restore(&self, ws: &Path, label: &str) -> Result<usize> {
        let target = self.load(label)?;
        let mut current = Manifest::new();
        walk(ws, ws, &mut |rel, _| {
            current.insert(rel.to_string(), String::new());
            Ok(())
        })?;
        let mut touched = 0;
        for rel in current.keys() {
            if !target.contains_key(rel) {
                let _ = std::fs::remove_file(ws.join(rel));
                touched += 1;
            }
        }
        for (rel, hash) in &target {
            let dst = ws.join(rel);
            let want = std::fs::read(self.obj(hash))?;
            if std::fs::read(&dst).ok().as_deref() != Some(&want[..]) {
                if let Some(p) = dst.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::write(&dst, want)?;
                touched += 1;
            }
        }
        Ok(touched)
    }

    /// Deletes the run's snapshots (after the run is accepted).
    pub fn prune(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&str, &Path) -> Result<()>) -> Result<()> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        let p = e.path();
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) || name == crate::brand::HANDOFF_DIR_NAME {
                continue;
            }
            walk(root, &p, f)?;
        } else if ft.is_file() {
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            f(&rel, &p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_and_restore_roundtrip() {
        let data = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let w = ws.path();
        std::fs::write(w.join("a.txt"), "one").unwrap();
        std::fs::create_dir_all(w.join("node_modules/x")).unwrap();
        std::fs::write(w.join("node_modules/x/big.js"), "skip me").unwrap();
        let s = SnapshotStore::new(data.path(), "run1");
        let base = s.take(w, "base").unwrap();
        assert!(base.contains_key("a.txt"));
        assert!(!base.keys().any(|k| k.starts_with("node_modules")));

        std::fs::write(w.join("a.txt"), "two").unwrap();
        std::fs::write(w.join("new.txt"), "new").unwrap();
        let s1 = s.take(w, "step-1").unwrap();
        assert_eq!(SnapshotStore::changed(&base, &s1), vec!["a.txt", "new.txt"]);

        s.restore(w, "base").unwrap();
        assert_eq!(std::fs::read_to_string(w.join("a.txt")).unwrap(), "one");
        assert!(!w.join("new.txt").exists());
        assert!(w.join("node_modules/x/big.js").exists());
        s.prune();
        assert!(s.load("base").is_err());
    }
}
