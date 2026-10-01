//! Installers: catalog-driven agent and runtime installs (the user sees and
//! confirms every command), the hardware check with Fits/Tight/Won't fit
//! model suggestions, model pulls with pause/resume/cancel, moving the model
//! folder, and GGUF import.

pub mod catalog;

pub use catalog::Catalog;

use crate::governor::{self, Fit};
use crate::macos;
use crate::proc::{self, CancelToken, ExitKind, ProcRegistry, RunOpts};
use anyhow::{bail, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Clone, Debug)]
pub struct HardwareCheck {
    pub hw: macos::Hardware,
    pub free_mem_gb: f64,
    pub budget_gb: f64,
    pub budget_pct: u8,
    pub model_folder: String,
    pub free_disk_gb: Option<f64>,
}

pub fn hardware_check(settings: &crate::settings::Settings) -> HardwareCheck {
    let hw = macos::hardware();
    let sys = governor::sys_view();
    let folder = crate::governor::service::OllamaService::models_dir(settings);
    HardwareCheck {
        budget_gb: (governor::budget_gb(hw.total_mem_gb, settings.memory_budget_pct) * 10.0).round() / 10.0,
        budget_pct: governor::budget_pct(hw.total_mem_gb, settings.memory_budget_pct),
        free_mem_gb: (sys.free_gb * 10.0).round() / 10.0,
        free_disk_gb: macos::free_disk_bytes(&folder).map(|b| (b as f64 / 1_073_741_824.0 * 10.0).round() / 10.0),
        model_folder: folder.to_string_lossy().into_owned(),
        hw,
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Suggestion {
    pub model: catalog::CatalogModel,
    pub fit: Fit,
    /// Context the governor would use on this Mac.
    pub ctx: u32,
    pub mem_gb: f64,
    pub installed: bool,
    pub best: bool,
    pub disk_ok: bool,
}

/// Catalog models rated for this machine. The best fit is the largest
/// model that "Fits"; installed models count as fitting first choices.
pub fn suggestions(cat: &Catalog, check: &HardwareCheck, installed: &[String]) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = cat
        .models
        .iter()
        .map(|m| {
            let ctx = governor::choose_ctx(m.download_gb, check.hw.total_mem_gb, check.budget_gb);
            let mem = if ctx >= 16_384 { m.mem_gb } else { governor::estimate_mem_gb(m.download_gb, ctx) };
            // Rate against the budget and the whole machine (not momentary free memory).
            let f = governor::fit(mem, check.hw.total_mem_gb * 0.75, check.budget_gb, false);
            Suggestion {
                installed: installed.iter().any(|i| i == &m.id || i.trim_end_matches(":latest") == m.id),
                disk_ok: check.free_disk_gb.is_none_or(|d| d > m.download_gb + 1.0),
                model: m.clone(),
                fit: f,
                ctx,
                mem_gb: mem,
                best: false,
            }
        })
        .collect();
    let best = out
        .iter()
        .enumerate()
        .filter(|(_, s)| s.fit == Fit::Fits && s.model.tool_calling)
        .max_by(|a, b| {
            (a.1.installed, a.1.model.params_b as i64).cmp(&(b.1.installed, b.1.model.params_b as i64))
        })
        .map(|(i, _)| i);
    if let Some(i) = best {
        out[i].best = true;
    }
    out
}

/// Runs a catalog install command in a login shell, streaming every line.
pub async fn run_install(command: &str, registry: &ProcRegistry, owner: &str, cancel: &CancelToken, mut on_line: impl FnMut(&str)) -> Result<()> {
    on_line(&format!("$ {command}"));
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = proc::command(&shell, false);
    cmd.args(["-lc", command]).env("NONINTERACTIVE", "1").env("HOMEBREW_NO_AUTO_UPDATE", "1").env("CI", "1");
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    let exit = proc::run_streaming(
        cmd,
        RunOpts { registry, owner, label: "installer", timeout: Duration::from_secs(1800), cancel },
        |_, l| on_line(&crate::adapters::strip_ansi(l)),
    )
    .await?;
    match exit {
        ExitKind::Exited(0) => Ok(()),
        ExitKind::Exited(c) => bail!("install command exited with {c}"),
        ExitKind::Cancelled => bail!("cancelled"),
        ExitKind::TimedOut => bail!("install timed out"),
        ExitKind::Signaled => bail!("install was interrupted"),
    }
}

/// Total size of a directory tree in bytes.
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn files_under(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = vec![];
    fn walk(d: &Path, out: &mut Vec<(PathBuf, u64)>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&p, out),
                Ok(t) if t.is_file() => out.push((p, e.metadata().map(|m| m.len()).unwrap_or(0))),
                _ => {}
            }
        }
    }
    walk(root, &mut out);
    out
}

/// Copies a model store to a new folder with progress, verifies sizes, then
/// removes the old copy. Resumable: files already copied with the right size
/// are skipped. Cancel leaves the old folder intact.
pub async fn move_models(from: &Path, to: &Path, cancel: &CancelToken, mut on: impl FnMut(u64, u64)) -> Result<()> {
    if from == to {
        bail!("that's already the model folder");
    }
    if to.starts_with(from) {
        bail!("the new folder can't be inside the current one");
    }
    let files = files_under(from);
    let total: u64 = files.iter().map(|f| f.1).sum();
    if let Some(free) = macos::free_disk_bytes(to) {
        if free < total {
            bail!("not enough space: need {:.1} GB, {:.1} GB free", total as f64 / 1e9, free as f64 / 1e9);
        }
    }
    let mut done = 0u64;
    for (src, size) in &files {
        if cancel.is_cancelled() {
            bail!("cancelled");
        }
        let rel = src.strip_prefix(from)?;
        let dst = to.join(rel);
        if dst.metadata().map(|m| m.len()) .ok() == Some(*size) {
            done += size;
            on(done, total);
            continue;
        }
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        let (s, d) = (src.clone(), dst.clone());
        tokio::task::spawn_blocking(move || std::fs::copy(&s, &d)).await??;
        done += size;
        on(done, total);
    }
    // Verify before deleting anything.
    for (src, size) in &files {
        let dst = to.join(src.strip_prefix(from)?);
        if dst.metadata().map(|m| m.len()).ok() != Some(*size) {
            bail!("verification failed for {}", dst.display());
        }
    }
    let f = from.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::remove_dir_all(f)).await??;
    Ok(())
}

/// Imports a GGUF file into Ollama under `name` via a Modelfile.
pub async fn import_gguf(gguf: &Path, name: &str, registry: &ProcRegistry, cancel: &CancelToken, on_line: impl FnMut(&str)) -> Result<()> {
    if gguf.extension().is_none_or(|e| e != "gguf") {
        bail!("choose a .gguf file");
    }
    let dir = std::env::temp_dir().join(format!("{}-import-{}", crate::brand::DATA_DIR_NAME, std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mf = dir.join("Modelfile");
    std::fs::write(&mf, format!("FROM {}\n", gguf.display()))?;
    let cmd = format!("ollama create {} -f {}", shell_quote(name), shell_quote(&mf.to_string_lossy()));
    let r = run_install(&cmd, registry, "import", cancel, on_line).await;
    let _ = std::fs::remove_dir_all(dir);
    r
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn move_models_copies_verifies_and_removes() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let from = a.path().join("models");
        std::fs::create_dir_all(from.join("blobs")).unwrap();
        std::fs::write(from.join("blobs/sha256-1"), vec![7u8; 4096]).unwrap();
        std::fs::create_dir_all(from.join("manifests/x")).unwrap();
        std::fs::write(from.join("manifests/x/latest"), "{}").unwrap();
        let to = b.path().join("new");
        let mut last = (0, 0);
        move_models(&from, &to, &CancelToken::default(), |d, t| last = (d, t)).await.unwrap();
        assert_eq!(last.0, last.1);
        assert!(!from.exists());
        assert_eq!(std::fs::read(to.join("blobs/sha256-1")).unwrap().len(), 4096);
    }

    #[test]
    fn suggestions_for_8gb_and_32gb() {
        let cat = Catalog::load(&catalog::dev_dir()).unwrap();
        let mk = |total: f64| HardwareCheck {
            hw: macos::Hardware { chip: "Apple M1".into(), total_mem_gb: total, cores: 8, apple_silicon: true, os: "macos".into() },
            free_mem_gb: total / 2.0,
            budget_gb: governor::budget_gb(total, None),
            budget_pct: governor::budget_pct(total, None),
            model_folder: "/tmp".into(),
            free_disk_gb: Some(100.0),
        };
        let s8 = suggestions(&cat, &mk(8.0), &[]);
        let best8 = s8.iter().find(|s| s.best).unwrap();
        assert_eq!(best8.model.id, "qwen2.5-coder:3b");
        assert_eq!(s8.iter().find(|s| s.model.id == "qwen2.5-coder:14b").unwrap().fit, Fit::WontFit);
        let s32 = suggestions(&cat, &mk(32.0), &[]);
        let best32 = s32.iter().find(|s| s.best).unwrap();
        assert!(best32.model.params_b >= 14.0, "{}", best32.model.id);
    }
}
