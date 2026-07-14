//! # vena-core
//!
//! The Vena engine: the knowledge **ledger**, the SQLite **store** that owns the
//! **gate**, the 5-stage spoiler-resistance **engine**, and the claim **verifier**
//! with the leak taxonomy. This crate is the moat (§2, §6).
//!
//! The kernel is inference-backend-agnostic: `ScriptedInference` powers unit tests
//! and offline runs; `OpenAiClient` powers **Cloud Relay**. Stage 1 (the gate) is
//! local SQL and runs before any backend is touched, so no ungated ledger content
//! can ever reach a remote endpoint (§11.4a Cloud Relay invariant).

pub mod engine;
pub mod error;
pub mod graph;
pub mod hash;
pub mod inference;
pub mod model;
pub mod pkg;
pub mod store;
pub mod util;
pub mod verify;
pub mod wiki;

pub use error::{Result, VenaError};
pub use model::*;
pub use store::Store;

#[cfg(test)]
mod tests;
