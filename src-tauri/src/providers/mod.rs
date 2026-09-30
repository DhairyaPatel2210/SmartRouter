//! Model backends: the Ollama HTTP API, OpenAI-compatible endpoints (LM
//! Studio, llama.cpp, MLX and every cheap-cloud provider), Anthropic's model
//! list, the Keychain for API keys, and a fake provider for tests.

pub mod fake;

use crate::proc::CancelToken;
use crate::types::ProviderType;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

pub fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(2)
            .user_agent(concat!("orchestrator/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client")
    })
}

// ---------------- Keychain ----------------

/// API keys live only in the OS keychain, under the app's bundle id.
pub mod keychain {
    use anyhow::Result;

    fn entry(key_ref: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(crate::brand::BUNDLE_ID, key_ref)?)
    }
    pub fn set(key_ref: &str, secret: &str) -> Result<()> {
        entry(key_ref)?.set_password(secret)?;
        Ok(())
    }
    pub fn get(key_ref: &str) -> Option<String> {
        entry(key_ref).ok()?.get_password().ok()
    }
    pub fn delete(key_ref: &str) -> Result<()> {
        match entry(key_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    /// "••••abcd" — the only form a key is ever shown in.
    pub fn hint(key_ref: &str) -> Option<String> {
        get(key_ref).map(|k| {
            let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("••••{tail}")
        })
    }
}

// ---------------- OpenAI-compatible & cloud ----------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CloudModel {
    pub id: String,
    pub name: String,
    pub ctx_len: Option<u32>,
    pub price_in_per_m: Option<f64>,
    pub price_out_per_m: Option<f64>,
    /// `None` = the provider doesn't say.
    pub tool_calling: Option<bool>,
}

fn auth(req: reqwest::RequestBuilder, p: ProviderType, key: Option<&str>) -> reqwest::RequestBuilder {
    match (p, key) {
        (ProviderType::Anthropic, Some(k)) => req.header("x-api-key", k).header("anthropic-version", "2023-06-01"),
        (_, Some(k)) => req.bearer_auth(k),
        _ => req,
    }
}

pub async fn list_models(p: ProviderType, base_url: &str, key: Option<&str>) -> Result<Vec<CloudModel>> {
    let base = base_url.trim_end_matches('/');
    let url = if p == ProviderType::Ollama { format!("{base}/api/tags") } else { format!("{base}/models") };
    let resp = auth(http().get(&url), p, key).timeout(Duration::from_secs(15)).send().await.context("provider unreachable")?;
    if resp.status() == 401 || resp.status() == 403 {
        bail!("The provider rejected the key ({}).", resp.status());
    }
    if !resp.status().is_success() {
        bail!("{url} returned {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await?;
    if p == ProviderType::Ollama {
        return Ok(v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m.get("name")?.as_str()?.to_string();
                Some(CloudModel { name: id.clone(), id, ctx_len: None, price_in_per_m: Some(0.0), price_out_per_m: Some(0.0), tool_calling: None })
            })
            .collect());
    }
    let per_m = |x: Option<&serde_json::Value>| -> Option<f64> {
        let x = x?;
        let f = x.as_f64().or_else(|| x.as_str()?.parse().ok())?;
        Some((f * 1_000_000.0 * 1e6).round() / 1e6)
    };
    let mut out: Vec<CloudModel> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            let name = m.get("name").or_else(|| m.get("display_name")).and_then(|n| n.as_str()).unwrap_or(&id).to_string();
            let tool_calling = m
                .get("supported_parameters")
                .and_then(|s| s.as_array())
                .map(|a| a.iter().any(|x| x.as_str() == Some("tools")));
            Some(CloudModel {
                ctx_len: m.get("context_length").or_else(|| m.get("context_window")).and_then(|c| c.as_u64()).map(|c| c as u32),
                price_in_per_m: per_m(m.pointer("/pricing/prompt")),
                price_out_per_m: per_m(m.pointer("/pricing/completion")),
                tool_calling,
                id,
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Checks a key with a free request where possible (model list / key info).
pub async fn test_key(p: ProviderType, base_url: &str, key: &str) -> Result<()> {
    let base = base_url.trim_end_matches('/');
    if p == ProviderType::Openrouter {
        let r = http().get(format!("{base}/key")).bearer_auth(key).timeout(Duration::from_secs(10)).send().await?;
        if r.status().is_success() {
            return Ok(());
        }
        bail!("OpenRouter rejected the key ({})", r.status());
    }
    list_models(p, base, Some(key)).await.map(|_| ())
}

/// A tiny chat request proving the model answers (the connect smoke test).
pub async fn smoke_test(p: ProviderType, base_url: &str, key: Option<&str>, model: &str) -> Result<String> {
    let base = base_url.trim_end_matches('/');
    let t = std::time::Instant::now();
    if p == ProviderType::Anthropic {
        let body = serde_json::json!({"model": model, "max_tokens": 8, "messages": [{"role":"user","content":"Reply with OK."}]});
        let r = auth(http().post(format!("{base}/messages")), p, key).json(&body).timeout(Duration::from_secs(20)).send().await?;
        if !r.status().is_success() {
            bail!("model returned {}", r.status());
        }
        return Ok(format!("Answered in {:.1}s", t.elapsed().as_secs_f64()));
    }
    let base = if p == ProviderType::Ollama && !base.ends_with("/v1") { format!("{base}/v1") } else { base.to_string() };
    let body = serde_json::json!({"model": model, "max_tokens": 8, "stream": false,
        "messages": [{"role":"user","content":"Reply with OK."}]});
    // Local models may need to load first; give them longer.
    let timeout = if p.is_local() { 120 } else { 20 };
    let r = auth(http().post(format!("{base}/chat/completions")), p, key)
        .json(&body)
        .timeout(Duration::from_secs(timeout))
        .send()
        .await
        .context("model did not answer")?;
    if !r.status().is_success() {
        let s = r.status();
        let text = r.text().await.unwrap_or_default();
        bail!("model returned {s}: {}", crate::adapters::truncate(&text, 200));
    }
    let v: serde_json::Value = r.json().await?;
    if v.pointer("/choices/0/message").is_none() {
        bail!("unexpected response from model");
    }
    Ok(format!("Answered in {:.1}s", t.elapsed().as_secs_f64()))
}

// ---------------- Ollama ----------------

pub const OLLAMA_DEFAULT: &str = "http://127.0.0.1:11434";

#[derive(Clone)]
pub struct Ollama {
    pub base: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub details: OllamaDetails,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OllamaDetails {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub parameter_size: String,
    #[serde(default)]
    pub quantization_level: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LoadedModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub context_length: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PullProgress {
    pub model: String,
    pub status: String,
    pub completed: u64,
    pub total: u64,
}

impl Ollama {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_string() }
    }

    pub async fn version(&self) -> Option<String> {
        let r = http().get(format!("{}/api/version", self.base)).timeout(Duration::from_millis(800)).send().await.ok()?;
        let v: serde_json::Value = r.json().await.ok()?;
        v.get("version").and_then(|x| x.as_str()).map(String::from)
    }

    pub async fn tags(&self) -> Result<Vec<OllamaModel>> {
        #[derive(Deserialize)]
        struct R {
            models: Vec<OllamaModel>,
        }
        let r: R = http().get(format!("{}/api/tags", self.base)).timeout(Duration::from_secs(5)).send().await?.json().await?;
        Ok(r.models)
    }

    pub async fn ps(&self) -> Result<Vec<LoadedModel>> {
        #[derive(Deserialize)]
        struct R {
            models: Vec<LoadedModel>,
        }
        let r: R = http().get(format!("{}/api/ps", self.base)).timeout(Duration::from_secs(3)).send().await?.json().await?;
        Ok(r.models)
    }

    /// Unloads a model now (keep_alive 0). Returns once Ollama accepted it.
    pub async fn unload(&self, model: &str) -> Result<()> {
        let body = serde_json::json!({"model": model, "keep_alive": 0});
        let r = http().post(format!("{}/api/generate", self.base)).json(&body).timeout(Duration::from_secs(15)).send().await?;
        if !r.status().is_success() {
            bail!("unload {model}: {}", r.status());
        }
        Ok(())
    }

    /// Loads a model with a context window, so memory is committed before the agent starts.
    pub async fn preload(&self, model: &str, num_ctx: Option<u32>, keep_alive: &str) -> Result<()> {
        let mut body = serde_json::json!({"model": model, "keep_alive": keep_alive, "prompt": ""});
        if let Some(c) = num_ctx {
            body["options"] = serde_json::json!({"num_ctx": c});
        }
        let r = http().post(format!("{}/api/generate", self.base)).json(&body).timeout(Duration::from_secs(300)).send().await?;
        if !r.status().is_success() {
            bail!("load {model}: {}", r.status());
        }
        Ok(())
    }

    pub async fn delete(&self, model: &str) -> Result<()> {
        let r = http()
            .delete(format!("{}/api/delete", self.base))
            .json(&serde_json::json!({"model": model}))
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        if !r.status().is_success() {
            bail!("delete {model}: {}", r.status());
        }
        Ok(())
    }

    /// Context length the model supports (from /api/show model_info).
    pub async fn max_context(&self, model: &str) -> Option<u32> {
        let r = http()
            .post(format!("{}/api/show", self.base))
            .json(&serde_json::json!({"model": model}))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        let v: serde_json::Value = r.json().await.ok()?;
        let info = v.get("model_info")?.as_object()?;
        info.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()).map(|c| c as u32)
    }

    /// Streams a pull. Ollama keeps partial blobs, so cancel = pause: the
    /// next pull resumes where it stopped.
    pub async fn pull(&self, model: &str, cancel: &CancelToken, mut on: impl FnMut(PullProgress)) -> Result<()> {
        let resp = http()
            .post(format!("{}/api/pull", self.base))
            .json(&serde_json::json!({"model": model, "stream": true}))
            .send()
            .await
            .context("Ollama is not reachable")?;
        if !resp.status().is_success() {
            bail!("pull {model}: {}", resp.status());
        }
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = cancel.cancelled() => bail!("cancelled"),
            };
            let Some(chunk) = chunk else { break };
            buf.extend_from_slice(&chunk?);
            while let Some(i) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&line) else { continue };
                if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                    return Err(anyhow!("{e}"));
                }
                on(PullProgress {
                    model: model.to_string(),
                    status: v.get("status").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    completed: v.get("completed").and_then(|c| c.as_u64()).unwrap_or(0),
                    total: v.get("total").and_then(|c| c.as_u64()).unwrap_or(0),
                });
            }
        }
        Ok(())
    }
}

/// Where Ollama keeps models: `$OLLAMA_MODELS` or `~/.ollama/models`.
pub fn ollama_default_models_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("OLLAMA_MODELS") {
        if !p.is_empty() {
            return p.into();
        }
    }
    dirs::home_dir().unwrap_or_default().join(".ollama/models")
}

/// Models already on disk in an Ollama store, read from manifests — works
/// even when the server isn't running, so we never re-download.
pub fn scan_ollama_store(dir: &std::path::Path) -> Vec<(String, u64)> {
    let lib = dir.join("manifests");
    let mut out = vec![];
    let Ok(registries) = std::fs::read_dir(&lib) else { return out };
    for reg in registries.flatten() {
        let Ok(namespaces) = std::fs::read_dir(reg.path()) else { continue };
        for ns in namespaces.flatten() {
            let Ok(models) = std::fs::read_dir(ns.path()) else { continue };
            for m in models.flatten() {
                let Ok(tags) = std::fs::read_dir(m.path()) else { continue };
                for t in tags.flatten() {
                    let model = m.file_name().to_string_lossy().to_string();
                    let tag = t.file_name().to_string_lossy().to_string();
                    let nsname = ns.file_name().to_string_lossy().to_string();
                    let name = if nsname == "library" { format!("{model}:{tag}") } else { format!("{nsname}/{model}:{tag}") };
                    let size = std::fs::read(t.path())
                        .ok()
                        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                        .map(|v| v["layers"].as_array().into_iter().flatten().filter_map(|l| l["size"].as_u64()).sum())
                        .unwrap_or(0);
                    out.push((name, size));
                }
            }
        }
    }
    out.sort();
    out
}

/// Other local runtimes (OpenAI-compatible) detected on their default ports.
#[derive(Serialize, Clone, Debug)]
pub struct DetectedRuntime {
    pub kind: ProviderType,
    pub endpoint: String,
    pub running: bool,
    pub models: Vec<String>,
}

pub async fn detect_openai_runtime(kind: ProviderType, endpoint: &str) -> DetectedRuntime {
    let r = http().get(format!("{endpoint}/models")).timeout(Duration::from_millis(600)).send().await;
    let models = match r {
        Ok(r) if r.status().is_success() => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .map(|v| v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default(),
        _ => return DetectedRuntime { kind, endpoint: endpoint.into(), running: false, models: vec![] },
    };
    DetectedRuntime { kind, endpoint: endpoint.into(), running: true, models }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_provider_lists_models_with_prices_and_tools() {
        let srv = fake::start(0).await.unwrap();
        let models = list_models(ProviderType::OpenaiCompatible, &srv.base_url(), Some(fake::FAKE_KEY)).await.unwrap();
        assert_eq!(models.len(), 2);
        let coder = models.iter().find(|m| m.id == "fake-coder").unwrap();
        assert_eq!(coder.tool_calling, Some(true));
        assert_eq!(coder.price_in_per_m, Some(0.2));
        assert_eq!(coder.ctx_len, Some(32768));
    }

    #[tokio::test]
    async fn bad_key_is_rejected_good_key_passes() {
        let srv = fake::start(0).await.unwrap();
        assert!(test_key(ProviderType::Openrouter, &srv.base_url(), "bad").await.is_err());
        assert!(test_key(ProviderType::Openrouter, &srv.base_url(), fake::FAKE_KEY).await.is_ok());
    }

    #[tokio::test]
    async fn smoke_test_against_fake_provider() {
        let srv = fake::start(0).await.unwrap();
        let r = smoke_test(ProviderType::OpenaiCompatible, &srv.base_url(), Some(fake::FAKE_KEY), "fake-coder").await;
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(srv.stats.chat_requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn scans_ollama_manifest_store() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("manifests/registry.ollama.ai/library/qwen2.5-coder");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("7b"), r#"{"layers":[{"size":4000},{"size":683}]}"#).unwrap();
        let found = scan_ollama_store(d.path());
        assert_eq!(found, vec![("qwen2.5-coder:7b".to_string(), 4683)]);
    }
}
