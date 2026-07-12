//! Domain types + IPC DTOs. These mirror the §11.2 TypeScript contract exactly so
//! the Tauri layer is a thin pass-through and the UI's `api.d.ts` stays in sync.

use serde::{Deserialize, Serialize};

/// `kind` of a ledger fact. Matches the schema CHECK-free enum in Appendix A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactKind {
    Event,
    Relationship,
    Secret,
    World,
    Death,
    Reveal,
}

impl FactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FactKind::Event => "event",
            FactKind::Relationship => "relationship",
            FactKind::Secret => "secret",
            FactKind::World => "world",
            FactKind::Death => "death",
            FactKind::Reveal => "reveal",
        }
    }
    pub fn parse(s: &str) -> FactKind {
        match s {
            "relationship" => FactKind::Relationship,
            "secret" => FactKind::Secret,
            "world" => FactKind::World,
            "death" => FactKind::Death,
            "reveal" => FactKind::Reveal,
            _ => FactKind::Event,
        }
    }
}

/// One entry in `fact.known_by_json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownBy {
    pub character_id: i64,
    pub learned_at_chapter: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: i64,
    pub story_id: i64,
    pub chapter_seq: i64,
    pub subject_char_id: Option<i64>,
    pub kind: FactKind,
    pub text: String,
    #[serde(default)]
    pub known_by: Vec<KnownBy>,
    pub spoiler_weight: i64,
}

/// Voice card stored in `character.voice_card_json` (§7 / Appendix B).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceCard {
    #[serde(default)]
    pub diction: String,
    #[serde(default)]
    pub temperament: String,
    #[serde(default)]
    pub speech_sample: String,
    /// "worldview AS OF this chapter" line, refined by the forge.
    #[serde(default)]
    pub worldview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: i64,
    pub story_id: i64,
    pub name: String,
    pub aliases: Vec<String>,
    pub voice_card: VoiceCard,
    pub first_appearance_chapter: i64,
    /// Convenience flag computed against current progress for the UI's silhouettes.
    #[serde(default)]
    pub met: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookMeta {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub author: Option<String>,
    pub license: String,
    pub source: Option<String>,
    pub cover: Option<String>,
    pub episode_count: i64,
    pub progress_episode: i64,
    /// 0..1 ledger-coverage score from the forge self-audit.
    pub ledger_coverage: f32,
    pub fact_count: i64,
    /// SHA of the package, surfaced in status rows ("SHA 77B1…E4").
    pub package_sha: Option<String>,
    /// prose | comic | illustrated-prose (format detection, §F5c).
    pub profile: String,
    pub forge_state: ForgeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeState {
    /// no ledger yet
    Raw,
    /// forging in progress
    Forging,
    /// ledger sealed and ready
    Sealed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeHtml {
    pub seq: i64,
    pub title: Option<String>,
    pub est_minutes: Option<i64>,
    pub content_html: String,
    pub scene_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theory {
    pub id: i64,
    pub text: String,
    pub logged_at_chapter: i64,
    /// None = open; "confirmed" | "busted"
    pub resolved_status: Option<String>,
    pub resolved_at_chapter: Option<i64>,
}

// ---- Companion turn reporting (leak taxonomy lives here) ----

/// Leak taxonomy from the mobile Test-the-Gate results (§11.4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeakKind {
    /// claim matches a fact with chapter_seq > progress
    FutureEvent,
    /// reply names a character with first_appearance > progress
    UnmetCharacter,
    /// reply's certainty/mood telegraphs the outcome (STRICT only, LLM-judged)
    ToneImpliesEnding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCheck {
    pub claim: String,
    /// "ok" | "violation" | "drift"
    pub verdict: String,
    pub leak_kind: Option<LeakKind>,
    /// id of the fact this claim matched, if any
    pub matched_fact_id: Option<i64>,
    pub score: f32,
}

/// TurnReport = {reply, repaired, redacted, claims:[...]} — §11.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnReport {
    pub reply: String,
    pub repaired: bool,
    pub redacted: bool,
    pub claims: Vec<ClaimCheck>,
    /// which leak kinds were caught this turn (for the shield tooltip)
    pub leaks_caught: Vec<LeakKind>,
}

/// Spoiler Gate dial (§11.4a). Persisted in `setting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum GateMode {
    Strict,
    #[default]
    Standard,
    Relaxed,
}

impl GateMode {
    /// Match threshold used by the verifier (lower = stricter matching).
    pub fn threshold(&self) -> f32 {
        match self {
            GateMode::Strict => 0.5,
            GateMode::Standard => 0.6,
            GateMode::Relaxed => 0.7,
        }
    }
    pub fn parse(s: &str) -> GateMode {
        match s {
            "strict" => GateMode::Strict,
            "relaxed" => GateMode::Relaxed,
            _ => GateMode::Standard,
        }
    }
}


/// Model-tier branding table (§11.4a). One config table, mapped to real GGUF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTier {
    pub id: &'static str,    // ink | quill | arch
    pub brand: &'static str, // INK·3B | QUILL·7B | ARCHIVIST·13B
    pub chip: &'static str,  // 3B | 7B | 13B
    pub gguf: &'static str,  // real model name
    pub size_gb: f32,
    pub min_ram_gb: u32,
}

pub const MODEL_TIERS: &[ModelTier] = &[
    ModelTier {
        id: "ink",
        brand: "INK·3B",
        chip: "3B",
        gguf: "Qwen3-4B-Instruct-Q4_K_M",
        size_gb: 1.9,
        min_ram_gb: 6,
    },
    ModelTier {
        id: "quill",
        brand: "QUILL·7B",
        chip: "7B",
        gguf: "Qwen3-8B-Instruct-Q4_K_M",
        size_gb: 4.6,
        min_ram_gb: 8,
    },
    ModelTier {
        id: "arch",
        brand: "ARCHIVIST·13B",
        chip: "13B",
        gguf: "Qwen3-14B-Instruct-Q4_K_M",
        size_gb: 7.9,
        min_ram_gb: 16,
    },
];
