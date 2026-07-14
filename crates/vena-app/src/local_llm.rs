//! In-process local inference (feature `embedded-llm`): the downloaded GGUF
//! tiers speak through llama.cpp compiled into the app — no Ollama, no
//! LM Studio, no separate server. This is what makes "download → chat" a
//! two-step story instead of a three-step one.
//!
//! Design constraints:
//! - The model loads lazily on first turn and stays resident (loading a
//!   multi-GB GGUF takes seconds; a turn must not pay that twice).
//! - One completion at a time: a single Mutex serializes turns. The engine's
//!   callers are interactive chat — parallel decodes would thrash memory.
//! - Prompting uses the model's own chat template from the GGUF metadata,
//!   falling back to ChatML (the Qwen family's native format).
//! - `is_remote()` is false: everything here stays on this device.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use vena_core::inference::{GenOptions, Inference};
use vena_core::{Result, VenaError};

const N_CTX: u32 = 8192;
const N_BATCH: usize = 1024;

fn backend() -> Result<&'static LlamaBackend> {
    static B: OnceLock<std::result::Result<LlamaBackend, String>> = OnceLock::new();
    match B.get_or_init(|| LlamaBackend::init().map_err(|e| e.to_string())) {
        Ok(b) => Ok(b),
        Err(e) => Err(VenaError::Inference(format!("llama.cpp init: {e}"))),
    }
}

/// The resident model — one at a time, keyed by path so switching tiers
/// (or deleting the active one) swaps/evicts it.
struct Resident {
    path: PathBuf,
    model: LlamaModel,
}

fn resident() -> &'static Mutex<Option<Resident>> {
    static M: OnceLock<Mutex<Option<Resident>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

/// Drop the resident model if it is the given path (delete_local_model calls
/// this so freed disk is matched by freed memory).
pub fn evict(path: &Path) {
    if let Ok(mut slot) = resident().lock() {
        if slot.as_ref().is_some_and(|r| r.path == path) {
            *slot = None;
        }
    }
}

pub struct EmbeddedLlm {
    path: PathBuf,
    label: String,
}

impl EmbeddedLlm {
    pub fn new(path: PathBuf, label: &str) -> Self {
        EmbeddedLlm {
            path,
            label: label.to_string(),
        }
    }
}

impl Inference for EmbeddedLlm {
    fn name(&self) -> String {
        format!("{} (on-device)", self.label)
    }

    fn is_remote(&self) -> bool {
        false
    }

    fn chat(
        &self,
        system: &str,
        turns: &[(String, String)],
        user: &str,
        opts: &GenOptions,
    ) -> Result<String> {
        self.run(system, turns, user, opts)
    }

    fn complete(&self, system: &str, user: &str, opts: &GenOptions) -> Result<String> {
        self.run(system, &[], user, opts)
    }
}

impl EmbeddedLlm {
    fn run(
        &self,
        system: &str,
        turns: &[(String, String)],
        user: &str,
        opts: &GenOptions,
    ) -> Result<String> {
        let be = backend()?;
        let mut slot = resident()
            .lock()
            .map_err(|_| VenaError::Inference("local engine lock poisoned".into()))?;

        // (Re)load if empty or a different tier was resident.
        if slot.as_ref().map(|r| r.path.as_path()) != Some(self.path.as_path()) {
            if !self.path.exists() {
                return Err(VenaError::Inference(format!(
                    "model weights missing at {}",
                    self.path.display()
                )));
            }
            // llama.cpp can crash on malformed files — refuse non-GGUF up front
            crate::gguf::assert_gguf(&self.path)
                .map_err(|e| VenaError::Inference(e.to_string()))?;
            let mut params = LlamaModelParams::default();
            if cfg!(target_os = "macos") {
                params = params.with_n_gpu_layers(1_000_000); // Metal: offload everything
            }
            let model = LlamaModel::load_from_file(be, &self.path, &params)
                .map_err(|e| VenaError::Inference(format!("loading local model: {e}")))?;
            *slot = Some(Resident {
                path: self.path.clone(),
                model,
            });
        }
        let model = &slot.as_ref().expect("just loaded").model;

        // Prompt via the model's own chat template; ChatML as fallback.
        let prompt = build_prompt(model, system, turns, user);

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_BATCH as u32);
        let mut ctx = model
            .new_context(be, ctx_params)
            .map_err(|e| VenaError::Inference(format!("llama.cpp context: {e}")))?;

        let mut tokens = model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| VenaError::Inference(format!("tokenize: {e}")))?;
        // Never let the prompt swallow the whole context: keep room to answer.
        let budget = (N_CTX as usize).saturating_sub(opts.max_tokens as usize + 16);
        if tokens.len() > budget {
            tokens.drain(0..tokens.len() - budget);
        }

        let mut batch = LlamaBatch::new(N_BATCH, 1);
        let mut pos: i32 = 0;
        let last_idx = tokens.len() - 1;
        for chunk in tokens.chunks(N_BATCH) {
            batch.clear();
            for tok in chunk.iter() {
                let is_last_of_prompt = pos as usize == last_idx;
                batch
                    .add(*tok, pos, &[0], is_last_of_prompt)
                    .map_err(|e| VenaError::Inference(format!("batch: {e}")))?;
                pos += 1;
            }
            ctx.decode(&mut batch)
                .map_err(|e| VenaError::Inference(format!("decode: {e}")))?;
        }

        let mut sampler = if opts.temperature <= 0.05 {
            LlamaSampler::greedy()
        } else {
            // seed from the clock — a fixed seed made two identical questions
            // produce the same reply, which reads as a broken record
            LlamaSampler::chain_simple([
                LlamaSampler::temp(opts.temperature),
                LlamaSampler::dist(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(42),
                ),
            ])
        };

        let mut out = String::new();
        // UTF-8 sequences can span tokens — a stateful decoder stitches them.
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        for _ in 0..opts.max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            if model.is_eog_token(token) {
                break;
            }
            if let Ok(piece) = model.token_to_piece(token, &mut decoder, false, None) {
                out.push_str(&piece);
            }
            if pos as u32 >= N_CTX - 1 {
                break; // context full — return what we have
            }
            batch.clear();
            batch
                .add(token, pos, &[0], true)
                .map_err(|e| VenaError::Inference(format!("batch: {e}")))?;
            pos += 1;
            ctx.decode(&mut batch)
                .map_err(|e| VenaError::Inference(format!("decode: {e}")))?;
        }

        Ok(strip_reasoning(out.trim()).to_string())
    }
}

/// Prefer the GGUF's own chat template — full multi-turn message list, the
/// way the model was trained to converse; ChatML (Qwen-native) as fallback.
fn build_prompt(
    model: &LlamaModel,
    system: &str,
    turns: &[(String, String)],
    user: &str,
) -> String {
    let msgs = || -> Option<Vec<LlamaChatMessage>> {
        let mut m = vec![LlamaChatMessage::new("system".into(), system.into()).ok()?];
        for (role, text) in turns {
            let r = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            m.push(LlamaChatMessage::new(r.into(), text.clone()).ok()?);
        }
        m.push(LlamaChatMessage::new("user".into(), user.into()).ok()?);
        Some(m)
    };
    if let (Ok(tmpl), Some(m)) = (model.chat_template(None), msgs()) {
        if let Ok(p) = model.apply_chat_template(&tmpl, &m, true) {
            return p;
        }
    }
    let mut p = format!("<|im_start|>system\n{system}<|im_end|>\n");
    for (role, text) in turns {
        let r = if role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        p.push_str(&format!("<|im_start|>{r}\n{text}<|im_end|>\n"));
    }
    p.push_str(&format!(
        "<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
    ));
    p
}

/// Reasoning-tuned models (Qwen3 included) may open with a <think> block.
/// The companion's voice is the ANSWER — thinking is never shown or verified.
fn strip_reasoning(s: &str) -> &str {
    if let Some(open) = s.find("<think>") {
        if let Some(close) = s[open..].find("</think>") {
            return s[open + close + "</think>".len()..].trim_start();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_weights_error_is_honest() {
        let llm = EmbeddedLlm::new(PathBuf::from("/nonexistent/model.gguf"), "INK·3B");
        let err = llm
            .complete("sys", "hi", &GenOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("missing"), "{err}");
    }

    #[test]
    fn reasoning_blocks_are_stripped() {
        assert_eq!(
            strip_reasoning("<think>secret plan</think>\nThe answer."),
            "The answer."
        );
        assert_eq!(strip_reasoning("plain"), "plain");
    }
}
