//! The shipped catalog (`catalog/*.json`): coding agents, local runtimes,
//! suggested local models and cloud providers.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstallOption {
    pub label: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CatalogAgent {
    pub id: String,
    pub display_name: String,
    pub family: String,
    #[serde(default)]
    pub tagline: String,
    pub bin: String,
    #[serde(default)]
    pub install: HashMap<String, Vec<InstallOption>>,
    #[serde(default)]
    pub uninstall: HashMap<String, String>,
    #[serde(default)]
    pub detect: String,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub headless: String,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub library: HashMap<String, String>,
    #[serde(default)]
    pub known_good_version: String,
    #[serde(default)]
    pub docs: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CatalogRuntime {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub endpoint: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub install: HashMap<String, Vec<InstallOption>>,
    #[serde(default)]
    pub docs: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub params_b: f64,
    pub quant: String,
    pub download_gb: f64,
    pub mem_gb: f64,
    pub ctx_max: u32,
    pub tool_calling: bool,
    pub runtime: String,
    #[serde(default)]
    pub good_for: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CatalogProvider {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub signup_url: String,
    #[serde(default)]
    pub key_prefix: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Catalog {
    pub agents: Vec<CatalogAgent>,
    pub runtimes: Vec<CatalogRuntime>,
    pub models: Vec<CatalogModel>,
    pub providers: Vec<CatalogProvider>,
    pub dir: PathBuf,
}

pub fn os_key() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// `catalog/` next to the sources (dev and tests).
pub fn dev_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../catalog")
}

impl Catalog {
    pub fn load(dir: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        struct A {
            agents: Vec<CatalogAgent>,
        }
        #[derive(Deserialize)]
        struct R {
            runtimes: Vec<CatalogRuntime>,
        }
        #[derive(Deserialize)]
        struct M {
            models: Vec<CatalogModel>,
        }
        #[derive(Deserialize)]
        struct P {
            providers: Vec<CatalogProvider>,
        }
        let read = |f: &str| std::fs::read(dir.join(f)).with_context(|| format!("catalog/{f}"));
        Ok(Self {
            agents: serde_json::from_slice::<A>(&read("agents.json")?)?.agents,
            runtimes: serde_json::from_slice::<R>(&read("runtimes.json")?)?.runtimes,
            models: serde_json::from_slice::<M>(&read("models.json")?)?.models,
            providers: serde_json::from_slice::<P>(&read("providers.json")?)?.providers,
            dir: dir.to_path_buf(),
        })
    }

    pub fn agent(&self, id: &str) -> Option<&CatalogAgent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn model(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn starter_dir(&self) -> PathBuf {
        self.dir.join("library-starter")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_catalog_parses_and_matches_adapters() {
        let c = Catalog::load(&dev_dir()).unwrap();
        for id in ["cursor", "claude", "codex", "copilot", "opencode", "aider", "goose"] {
            let a = c.agent(id).unwrap_or_else(|| panic!("{id} missing from catalog"));
            assert!(a.install.contains_key("macos"), "{id} has no macOS install");
        }
        assert!(c.runtimes.iter().any(|r| r.id == "ollama" && r.default));
        assert!(c.models.iter().all(|m| m.tool_calling));
        assert!(c.providers.iter().any(|p| p.id == "openrouter"));
        assert!(c.starter_dir().join("agents/test-writer.md").exists());
    }
}
