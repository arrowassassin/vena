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
    /// Multi-turn chat: `turns` are (role, text) pairs oldest-first with roles
    /// "user"/"assistant". Backends with native chat formats override this —
    /// the default flattens into the single-shot prompt so every backend works.
    fn chat(
        &self,
        system: &str,
        turns: &[(String, String)],
        user: &str,
        opts: &GenOptions,
    ) -> Result<String> {
        if turns.is_empty() {
            return self.complete(system, user, opts);
        }
        let mut sys = system.to_string();
        sys.push_str(
            "\n\n== EARLIER IN THIS CONVERSATION (stay consistent; do not repeat verbatim) ==\n",
        );
        for (role, text) in turns {
            sys.push_str(if role == "user" { "READER: " } else { "YOU: " });
            let line: String = text.chars().take(400).collect();
            sys.push_str(&line);
            sys.push('\n');
        }
        self.complete(&sys, user, opts)
    }
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
        // Normalize the base to its ROOT (no trailing slash, no trailing `/v1`) so a
        // user or preset can supply either `https://host/api` OR `https://host/api/v1`
        // — the endpoint methods add exactly one `/v1/...`. (OpenAI-compatible servers
        // and every relay preset advertise a base that includes `/v1`; appending
        // another `/v1` produced `/v1/v1/chat/completions` and 404'd every request.)
        let base = base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_string();
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

    /// The fully-resolved chat endpoint — exactly one `/v1/chat/completions`.
    pub fn chat_endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
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

/// Native Anthropic Messages API client (`/v1/messages`). Anthropic keys are a
/// first-class Cloud Relay option alongside OpenAI-compatible endpoints — same
/// invariant: it only ever receives the already-gated prompt.
pub struct AnthropicClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::blocking::Client,
}

impl AnthropicClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        AnthropicClient {
            // Anthropic's endpoint is `{base}/v1/messages`; tolerate a base given with
            // or without a trailing `/v1` (same normalization as OpenAiClient).
            base_url: base_url
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .trim_end_matches('/')
                .to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("http client"),
        }
    }
}

impl Inference for AnthropicClient {
    fn name(&self) -> String {
        format!("cloud-relay(anthropic):{}", self.model)
    }
    fn is_remote(&self) -> bool {
        true
    }
    fn complete(&self, system: &str, user: &str, opts: &GenOptions) -> Result<String> {
        let mut sys = system.to_string();
        if opts.json {
            sys.push_str("\n\nRespond with STRICT JSON only — no prose, no code fences.");
        }
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": opts.max_tokens,
            "temperature": opts.temperature,
            "system": sys,
            "messages": [{"role": "user", "content": user}],
        });
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(VenaError::Inference(format!(
                "anthropic backend returned {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        let text = v["content"][0]["text"]
            .as_str()
            .ok_or_else(|| VenaError::Inference("no content in anthropic response".into()))?
            .to_string();
        Ok(text)
    }
}

/// Resolve the dev/eval backend from env. Priority: ANTHROPIC_API_KEY (native
/// Anthropic endpoint) → VENA_BASE_URL (OpenAI-compat, incl. localhost servers).
pub fn backend_from_env() -> Option<(String, Box<dyn Inference>)> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        let base = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".into());
        let model =
            std::env::var("VENA_MODEL").unwrap_or_else(|_| "claude-haiku-4-5-20251001".into());
        return Some((
            format!("{base} ({model})"),
            Box::new(AnthropicClient::new(&base, &key, &model)),
        ));
    }
    let base = std::env::var("VENA_BASE_URL").ok()?;
    let key = std::env::var("VENA_API_KEY").unwrap_or_default();
    let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    Some((
        format!("{base} ({model})"),
        Box::new(OpenAiClient::new(&base, &key, &model)),
    ))
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
            .post(self.chat_endpoint())
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

#[cfg(test)]
mod url_tests {
    use super::OpenAiClient;

    #[test]
    fn chat_endpoint_never_doubles_v1() {
        // Base WITH /v1 (every relay preset + most OpenAI-compat servers advertise this).
        for base in [
            "https://openrouter.ai/api/v1",
            "https://openrouter.ai/api/v1/",
            "http://localhost:11434/v1",
            "https://api.groq.com/openai/v1",
        ] {
            let c = OpenAiClient::new(base, "k", "m");
            let url = c.chat_endpoint();
            assert!(
                !url.contains("/v1/v1/"),
                "doubled /v1 for base {base}: {url}"
            );
            assert!(url.ends_with("/v1/chat/completions"), "{url}");
        }
        // Base WITHOUT /v1 also resolves to exactly one.
        let c = OpenAiClient::new("https://openrouter.ai/api", "k", "m");
        assert_eq!(
            c.chat_endpoint(),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }
}
