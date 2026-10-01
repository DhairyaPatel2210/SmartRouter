//! The app-managed Ollama service. Started only while the app needs it
//! (unless the user sets it to always run), with one model loaded at a time,
//! idle unload, the chosen models folder and lowered priority. A server the
//! user started themselves is used as-is and never stopped by us.

use crate::proc::{self, ProcRegistry};
use crate::providers::{Ollama, OLLAMA_DEFAULT};
use crate::settings::Settings;
use anyhow::{bail, Result};
use parking_lot::Mutex;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;

#[derive(Default)]
pub struct OllamaService {
    child: Mutex<Option<Child>>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub managed_by_app: bool,
    pub version: Option<String>,
    pub endpoint: String,
    pub models_dir: String,
    /// Shown when the user runs Ollama themselves and the folder differs.
    pub folder_hint: Option<String>,
}

impl OllamaService {
    pub fn client(&self) -> Ollama {
        Ollama::new(std::env::var("OLLAMA_HOST").ok().filter(|h| h.starts_with("http")).unwrap_or_else(|| OLLAMA_DEFAULT.into()))
    }

    pub fn managed(&self) -> bool {
        self.child.lock().is_some()
    }

    pub fn models_dir(settings: &Settings) -> std::path::PathBuf {
        settings.model_folder.clone().map(Into::into).unwrap_or_else(crate::providers::ollama_default_models_dir)
    }

    pub async fn status(&self, settings: &Settings) -> OllamaStatus {
        let c = self.client();
        let version = c.version().await;
        let dir = Self::models_dir(settings);
        let managed = self.managed();
        let folder_hint = match (&version, managed, &settings.model_folder) {
            (Some(_), false, Some(f)) if *f != crate::providers::ollama_default_models_dir().to_string_lossy() => Some(format!(
                "Ollama is running outside the app. To use your chosen folder, quit Ollama and set OLLAMA_MODELS=\"{f}\" (or let the app start Ollama)."
            )),
            _ => None,
        };
        OllamaStatus {
            installed: proc::which("ollama").is_some(),
            running: version.is_some(),
            managed_by_app: managed,
            version,
            endpoint: c.base.clone(),
            models_dir: dir.to_string_lossy().into_owned(),
            folder_hint,
        }
    }

    /// Makes sure a server is reachable, starting one if allowed.
    /// Returns true if this call started it.
    pub async fn ensure_running(&self, settings: &Settings, registry: &ProcRegistry, total_gb: f64) -> Result<bool> {
        let c = self.client();
        if c.version().await.is_some() {
            return Ok(false);
        }
        if !settings.manage_ollama {
            bail!("Ollama isn't running. Start it, or allow the app to start it in Settings → Models.");
        }
        let Some(bin) = proc::which("ollama") else { bail!("Ollama isn't installed. Install it from Agents & Models.") };
        let mut cmd = proc::command(&bin, settings.lower_priority);
        let dir = Self::models_dir(settings);
        let _ = std::fs::create_dir_all(&dir);
        let budget = super::budget_gb(total_gb, settings.memory_budget_pct);
        // Context default: 16k on ≤16 GB Macs, 32k above (per-model caps are applied on load).
        let ctx = if total_gb <= 16.5 { 16_384 } else { 32_768 };
        cmd.arg("serve")
            .env("OLLAMA_MODELS", &dir)
            .env("OLLAMA_MAX_LOADED_MODELS", "1")
            .env("OLLAMA_NUM_PARALLEL", "1")
            .env("OLLAMA_KEEP_ALIVE", format!("{}m", settings.idle_unload_minutes.max(1)))
            .env("OLLAMA_CONTEXT_LENGTH", ctx.to_string())
            .env("OLLAMA_GPU_OVERHEAD", "0")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = budget;
        let child = cmd.spawn()?;
        if let Some(pid) = child.id() {
            registry.add(pid, "ollama", "Ollama (managed by this app)");
        }
        *self.child.lock() = Some(child);
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if c.version().await.is_some() {
                return Ok(true);
            }
        }
        self.stop(registry).await;
        bail!("Ollama didn't start within 10 seconds")
    }

    /// Stops the server only if this app started it.
    pub async fn stop(&self, registry: &ProcRegistry) {
        let child = self.child.lock().take();
        if let Some(mut c) = child {
            if let Some(pid) = c.id() {
                proc::kill_tree(pid).await;
                registry.remove(pid);
            }
            let _ = c.kill().await;
        }
    }

    /// Blocking stop for app exit.
    pub fn stop_blocking(&self) {
        if let Some(mut c) = self.child.lock().take() {
            if let Some(pid) = c.id() {
                #[cfg(unix)]
                unsafe {
                    libc::killpg(pid as i32, libc::SIGTERM);
                }
            }
            let _ = c.start_kill();
        }
    }
}
