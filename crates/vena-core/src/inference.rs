//! Inference abstraction (§4). One trait, two backends: a local GGUF runner
//! (represented here by a deterministic stand-in until llama.cpp is wired) and an
//! OpenAI-compatible client that powers **Cloud Relay** (BYO key).
//!
//! Invariant (§11.4a): a remote backend only ever receives the *already-gated*
//! system prompt + conversation. Stage 1 (the gate) is local SQL and runs before
//! any backend is called — this module never sees ungated ledger content.

use crate::error::{Result, VenaError};
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
pub struct ScriptedInference {
    replies: Mutex<std::collections::VecDeque<String>>,
    fallback: String,
    name: String,
}

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
/// LM Studio / ollama / the user's own proxy. This is **Cloud Relay**.
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::blocking::Client,
}

impl OpenAiClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        OpenAiClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("http client"),
        }
    }
}

impl Inference for OpenAiClient {
    fn name(&self) -> String {
        format!("cloud-relay:{}", self.model)
    }
    fn is_remote(&self) -> bool {
        true
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
