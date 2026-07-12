//! Inference abstraction (§4). One trait, one real client:
//! `OpenAiClient` speaks the OpenAI-compatible `/v1/chat/completions` protocol and
//! covers every backend the product uses — **Cloud Relay** (BYO key, remote) AND a
//! *local* GGUF served by a bundled llama.cpp / ollama / LM Studio server on
//! localhost (on-device; `is_remote()` returns false). One protocol, no per-backend
//! SDKs, no fragile FFI on every build.
//!
//! Invariant (§11.4a): a remote backend only ever receives the *already-gated*
//! system prompt + conversation. Stage 1 (the gate) is local SQL and runs before
//! any backend is called — this module never sees ungated ledger content. The
//! `ScriptedInference` mock is compiled ONLY under `cfg(test)`/`testkit`; it never
//! ships in the app.

use crate::error::{Result, VenaError};
#[cfg(any(test, feature = "testkit"))]
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct GenOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    /// Request strict-JSON output (used by the stage-4 claim extractor and forge).
    pub json: bool,
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions {
            max_tokens: 512,
            temperature: 0.7,
            json: false,
        }
    }
}

pub trait Inference: Send + Sync {
    fn name(&self) -> String;
    /// True for Cloud Relay / any endpoint off the device. Used to assert the
    /// "nothing ungated leaves the device" invariant at the call site.
    fn is_remote(&self) -> bool;
    fn complete(&self, system: &str, user: &str, opts: &GenOptions) -> Result<String>;
}

/// Deterministic backend for unit tests and offline Phase-1 CLI runs. Replies are
/// scripted by the caller; when the queue drains it echoes a neutral, gate-safe
/// line. This lets the gate + verify stages be tested end-to-end with no model.
///
/// TEST/DEV ONLY — gated behind `cfg(test)`/the `testkit` feature so it can never be
/// wired into the shipped app (§ "no mocks in runtime"). The app uses `OpenAiClient`
/// (Cloud Relay / local OpenAI-compat server) or the embedded `LocalLlama` backend.
#[cfg(any(test, feature = "testkit"))]
pub struct ScriptedInference {
    replies: Mutex<std::collections::VecDeque<String>>,
    fallback: String,
    name: String,
}

#[cfg(any(test, feature = "testkit"))]
impl ScriptedInference {
    pub fn new(replies: Vec<String>) -> Self {
        ScriptedInference {
            replies: Mutex::new(replies.into_iter().collect()),
            fallback: "I have not lived that part of the tale yet.".to_string(),
            name: "scripted-mock".to_string(),
        }
    }
    pub fn with_fallback(mut self, f: &str) -> Self {
        self.fallback = f.to_string();
        self
    }
}

#[cfg(any(test, feature = "testkit"))]
impl Inference for ScriptedInference {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn is_remote(&self) -> bool {
        false
    }
    fn complete(&self, _system: &str, _user: &str, _opts: &GenOptions) -> Result<String> {
        let mut q = self.replies.lock().unwrap();
        Ok(q.pop_front().unwrap_or_else(|| self.fallback.clone()))
    }
}

/// OpenAI-compatible client — one interface covers OpenRouter / Gemini-compat /
/// LM Studio / ollama / the user's own proxy. This is **Cloud Relay** when the
/// endpoint is remote; the SAME client also drives a *local* OpenAI-compat server
/// (ollama / LM Studio on localhost), which is on-device and therefore NOT remote.
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    remote: bool,
    http: reqwest::blocking::Client,
}

impl OpenAiClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        OpenAiClient {
            remote: Self::is_remote_host(&base),
            base_url: base,
            api_key: api_key.to_string(),
            model: model.to_string(),
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("http client"),
        }
    }

    /// A URL is on-device only if it targets loopback. Everything else is remote and
    /// the Cloud Relay invariant applies (no ungated content may be sent).
    fn is_remote_host(base_url: &str) -> bool {
        let host = base_url
            .split("://")
            .nth(1)
            .unwrap_or(base_url)
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        !matches!(
            host.as_str(),
            "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]"
        )
    }
}

impl Inference for OpenAiClient {
    fn name(&self) -> String {
        if self.remote {
            format!("cloud-relay:{}", self.model)
        } else {
            format!("local-server:{}", self.model)
        }
    }
    fn is_remote(&self) -> bool {
        self.remote
    }
    fn complete(&self, system: &str, user: &str, opts: &GenOptions) -> Result<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "temperature": opts.temperature,
            "max_tokens": opts.max_tokens,
        });
        if opts.json {
            body["response_format"] = serde_json::json!({"type": "json_object"});
        }
        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(VenaError::Inference(format!(
                "backend returned {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| VenaError::Inference("no content in response".into()))?
            .to_string();
        Ok(text)
    }
}
