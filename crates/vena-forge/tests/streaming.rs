//! The streaming forge unit: extract_chapter yields one chapter's slice and carries
//! the character roster forward, so the app can insert per chapter and make early
//! chapters chat-ready while later ones forge (§6 gate is per-fact).

use std::sync::Mutex;
use vena_core::inference::{GenOptions, Inference};
use vena_core::Result;
use vena_forge::import::Chapter;
use vena_forge::ledger::extract_chapter;

/// Inline mock: returns a canned per-chapter ledger JSON, one queued reply per call.
struct MockLedgerModel {
    replies: Mutex<Vec<String>>,
}
impl Inference for MockLedgerModel {
    fn name(&self) -> String {
        "mock".into()
    }
    fn is_remote(&self) -> bool {
        false
    }
    fn complete(&self, _system: &str, _user: &str, _opts: &GenOptions) -> Result<String> {
        Ok(self.replies.lock().unwrap().remove(0))
    }
}

#[test]
fn extract_chapter_slices_and_carries_roster() {
    let model = MockLedgerModel {
        replies: Mutex::new(vec![
            // ch1: introduces Alice + a fact
            r#"{"facts":[{"text":"Alice enters the manor","kind":"event","subject":"Alice","known_by":[{"character":"Alice","learned_this_chapter":true}],"spoiler_weight":1}],"new_characters":[{"name":"Alice","aliases":["Al"],"voice":{"diction":"x","temperament":"y","speech_sample":"z"}}]}"#.into(),
            // ch2: Alice already known; introduces Bob
            r#"{"facts":[{"text":"Bob is revealed as the culprit","kind":"reveal","subject":"Bob","known_by":[{"character":"Bob","learned_this_chapter":true}],"spoiler_weight":3}],"new_characters":[{"name":"Bob","aliases":[],"voice":{"diction":"a","temperament":"b","speech_sample":"c"}}]}"#.into(),
        ]),
    };
    let ch1 = Chapter {
        seq: 1,
        title: None,
        paragraphs: vec!["Alice.".into()],
    };
    let ch2 = Chapter {
        seq: 2,
        title: None,
        paragraphs: vec!["Bob.".into()],
    };

    let mut known: Vec<String> = Vec::new();
    let p1 = extract_chapter(&model, &ch1, &mut known).unwrap();
    assert_eq!(p1.characters.len(), 1, "ch1 introduces Alice");
    assert_eq!(p1.facts.len(), 1);
    assert_eq!(p1.facts[0].chapter, 1);
    assert!(
        known.iter().any(|k| k == "Alice"),
        "roster carries Alice forward"
    );

    let p2 = extract_chapter(&model, &ch2, &mut known).unwrap();
    // Alice is already known → only Bob is a NEW character this chapter.
    assert_eq!(p2.characters.len(), 1, "only Bob is new in ch2");
    assert_eq!(p2.characters[0].name, "Bob");
    assert_eq!(p2.facts[0].chapter, 2);
    assert_eq!(p2.facts[0].spoiler_weight, 3, "reveal weight preserved");
}
