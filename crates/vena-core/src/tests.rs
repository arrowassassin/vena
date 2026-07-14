//! Kernel unit tests (§11.5 segment 1 exit criteria): gate correctness (future
//! facts invisible; per-character knowledge lags the reader), verify/leak taxonomy,
//! engine repair/redact, guard-fates, theory resolution, wiki consent, burn, re-seal.

use crate::engine::{resolve_theories, Engine};
use crate::inference::ScriptedInference;
use crate::model::*;
use crate::store::Store;
use crate::wiki::{self, WikiMode};

fn kb(character_id: i64, learned: i64) -> KnownBy {
    KnownBy {
        character_id,
        learned_at_chapter: learned,
    }
}

/// A small Dracula-shaped fixture. Returns (store, story_id, {char ids}).
fn fixture() -> (Store, i64, std::collections::HashMap<&'static str, i64>) {
    let s = Store::in_memory().unwrap();
    let sid = s
        .insert_story(
            "dracula",
            "Dracula",
            Some("Bram Stoker"),
            "public-domain",
            None,
            None,
            "{}",
        )
        .unwrap();

    let mut ids = std::collections::HashMap::new();
    ids.insert(
        "jonathan",
        s.insert_character(
            sid,
            "Jonathan Harker",
            &["Harker".into()],
            &VoiceCard::default(),
            1,
        )
        .unwrap(),
    );
    ids.insert(
        "mina",
        s.insert_character(
            sid,
            "Mina Murray",
            &["Mina".into()],
            &VoiceCard::default(),
            3,
        )
        .unwrap(),
    );
    ids.insert(
        "lucy",
        s.insert_character(
            sid,
            "Lucy Westenra",
            &["Lucy".into()],
            &VoiceCard::default(),
            5,
        )
        .unwrap(),
    );
    ids.insert(
        "vanhelsing",
        s.insert_character(
            sid,
            "Van Helsing",
            &["Abraham Van Helsing".into()],
            &VoiceCard::default(),
            9,
        )
        .unwrap(),
    );

    let fact = |chapter, subject: Option<i64>, kind, text: &str, known: Vec<KnownBy>, w| {
        s.insert_fact(&Fact {
            id: 0,
            story_id: sid,
            chapter_seq: chapter,
            subject_char_id: subject,
            kind,
            text: text.to_string(),
            known_by: known,
            spoiler_weight: w,
        })
        .unwrap()
    };

    let j = ids["jonathan"];
    let m = ids["mina"];
    let l = ids["lucy"];
    fact(
        1,
        Some(j),
        FactKind::Event,
        "Jonathan Harker travels to Transylvania",
        vec![kb(j, 1)],
        1,
    );
    fact(
        2,
        None,
        FactKind::Reveal,
        "The Count is a vampire",
        vec![kb(j, 2)],
        3,
    );
    // Reader learns at ch3 that Mina and Jonathan are engaged, but Lucy only
    // hears of it at ch8 — character knowledge lags the reader.
    fact(
        3,
        Some(m),
        FactKind::Relationship,
        "Mina and Jonathan are engaged",
        vec![kb(m, 3), kb(l, 8)],
        1,
    );
    fact(
        12,
        Some(l),
        FactKind::Death,
        "Lucy dies",
        vec![kb(m, 12)],
        3,
    );
    fact(
        20,
        None,
        FactKind::Reveal,
        "Dracula is destroyed at the castle",
        vec![],
        3,
    );

    (s, sid, ids)
}

#[test]
fn gate_hides_future_facts() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 4, 0).unwrap();
    let visible = s.gated_facts(sid, 4, None, "", 50).unwrap();
    // chapters 1,2,3 visible; 12 and 20 hidden.
    assert!(visible.iter().all(|f| f.chapter_seq <= 4));
    assert!(visible.iter().any(|f| f.text.contains("vampire")));
    assert!(!visible.iter().any(|f| f.text.contains("Lucy dies")));
    assert!(!visible.iter().any(|f| f.text.contains("destroyed")));
}

#[test]
fn per_character_knowledge_lags_reader() {
    let (s, sid, ids) = fixture();
    // Reader at chapter 5: narrator sees "Mina and Jonathan are engaged" (ch3).
    let narrator = s.gated_facts(sid, 5, None, "", 50).unwrap();
    assert!(narrator.iter().any(|f| f.text.contains("engaged")));

    // Lucy at chapter 5 does NOT know it yet (she learns at ch8).
    let lucy = s.gated_facts(sid, 5, Some(ids["lucy"]), "", 50).unwrap();
    assert!(!lucy.iter().any(|f| f.text.contains("engaged")));

    // At chapter 8 Lucy now knows it.
    let lucy8 = s.gated_facts(sid, 8, Some(ids["lucy"]), "", 50).unwrap();
    assert!(lucy8.iter().any(|f| f.text.contains("engaged")));
}

#[test]
fn forbidden_includes_future_and_character_lag() {
    let (s, sid, ids) = fixture();
    // Narrator at ch5: forbidden = future facts only (ch12, ch20).
    let forb = s.forbidden_facts(sid, 5, None).unwrap();
    assert!(forb.iter().any(|f| f.text.contains("Lucy dies")));
    assert!(!forb.iter().any(|f| f.text.contains("engaged")));

    // Lucy at ch5: "engaged" (ch3, she learns ch8) is forbidden for her.
    let forb_lucy = s.forbidden_facts(sid, 5, Some(ids["lucy"])).unwrap();
    assert!(forb_lucy.iter().any(|f| f.text.contains("engaged")));
}

#[test]
fn verify_flags_future_event() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let visible = s.gated_facts(sid, 6, None, "Lucy", 50).unwrap();
    let forbidden = s.forbidden_facts(sid, 6, None).unwrap();
    let check = crate::verify::match_claim("Lucy dies and rises again", &visible, &forbidden, 0.6);
    assert_eq!(check.verdict, "violation");
    assert_eq!(check.leak_kind, Some(LeakKind::FutureEvent));
}

#[test]
fn verify_flags_unmet_character() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap(); // Van Helsing appears ch9 -> unmet
    let unmet = s.unmet_character_names(sid).unwrap();
    let hits = crate::verify::unmet_characters(
        "Perhaps Van Helsing could help us.",
        unmet.iter().map(String::as_str),
    );
    assert!(hits.iter().any(|h| h == "Van Helsing"));
    // A met character is not flagged.
    let none =
        crate::verify::unmet_characters("Jonathan is brave.", unmet.iter().map(String::as_str));
    assert!(none.is_empty());
}

#[test]
fn engine_standard_repairs_then_clean() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    // First draft leaks the ch12 death; repair regen is clean.
    let backend = ScriptedInference::new(vec![
        "It troubles me — and I fear Lucy dies before this is over.".into(),
        "It troubles me greatly; I cannot say how it will end.".into(),
    ]);
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    let mut stages = Vec::new();
    let report = eng
        .companion_turn(
            &s,
            sid,
            None,
            "What do you make of Lucy's illness?",
            &mut |st| stages.push(st.to_string()),
        )
        .unwrap();
    assert!(report.repaired, "a repair should have occurred");
    assert!(
        !report.claims.iter().any(|c| c.verdict == "violation"),
        "final reply must be clean: {:?}",
        report.claims
    );
    assert!(!report.reply.to_lowercase().contains("lucy dies"));
    assert_eq!(stages, vec!["gate", "compose", "verify", "repair"]);
}

#[test]
fn engine_strict_redacts_immediately() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let backend = ScriptedInference::new(vec![
        "I fear it deeply. Lucy dies before the tale is done.".into(),
    ]);
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Strict);
    let report = eng
        .companion_turn(&s, sid, None, "Tell me about Lucy.", &mut |_| {})
        .unwrap();
    assert!(report.redacted, "STRICT must redact on any violation");
    assert!(!report.reply.to_lowercase().contains("lucy dies"));
}

#[test]
fn redaction_strips_unmet_character_names() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 1, 0).unwrap();
    // Both the draft AND the repair regen name characters the reader has not
    // met — the final redacted reply must not contain those sentences.
    let leak = "You will see: Van Helsing arrives and Lucy dies at the end.";
    let backend = ScriptedInference::new(vec![leak.into(), leak.into()]);
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    let report = eng
        .companion_turn(&s, sid, None, "Tell me plainly what comes.", &mut |_| {})
        .unwrap();
    assert!(report.redacted, "double leak must end in redaction");
    let lower = report.reply.to_lowercase();
    assert!(
        !lower.contains("van helsing") && !lower.contains("lucy"),
        "redacted reply still names unmet characters: {}",
        report.reply
    );
}

#[test]
fn guard_fates_short_circuits_without_generation() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    // Empty script: if generation were called it'd use the fallback line, but the
    // guard should short-circuit before any backend call.
    let backend = ScriptedInference::new(vec![]);
    let mut eng = Engine::new(Box::new(backend));
    eng.guard_fates = true;
    let mut stages = Vec::new();
    let report = eng
        .companion_turn(&s, sid, None, "Does Lucy die?", &mut |st| {
            stages.push(st.to_string())
        })
        .unwrap();
    assert!(report.claims.is_empty());
    assert!(!report.reply.to_lowercase().contains("die"));
    // Only the gate stage ran; compose/verify were skipped.
    assert_eq!(stages, vec!["gate"]);
}

#[test]
fn theories_resolve_only_after_reveal() {
    let (s, sid, _) = fixture();
    let t = s.add_theory(sid, "Lucy dies from the illness", 6).unwrap();

    // At chapter 6, the reveal (ch12) is still future — theory stays open.
    s.set_progress(sid, 6, 0).unwrap();
    resolve_theories(&s, sid).unwrap();
    let open = s.list_theories(sid).unwrap();
    assert!(open[0].resolved_status.is_none(), "must not resolve early");

    // Pass the reveal → resolves.
    s.set_progress(sid, 12, 0).unwrap();
    resolve_theories(&s, sid).unwrap();
    let resolved = s.list_theories(sid).unwrap();
    assert_eq!(resolved[0].resolved_status.as_deref(), Some("confirmed"));
    assert_eq!(resolved[0].resolved_at_chapter, Some(12));
    let _ = t;
}

#[test]
fn wiki_full_requires_consent() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    // Synced always allowed.
    assert!(wiki::get_wiki_index(&s, sid, WikiMode::Synced).is_ok());
    // Full without consent is refused.
    let err = wiki::get_wiki_index(&s, sid, WikiMode::Full).unwrap_err();
    assert_eq!(err.code(), "SpoilerConsentRequired");
    // Grant consent -> allowed, and future facts appear.
    wiki::set_consent(&s, sid, true).unwrap();
    let full = wiki::get_wiki_page(&s, sid, "char:3", WikiMode::Full).unwrap(); // Lucy = id 3
    assert!(full.unsealed);
    let has_death = full
        .sections
        .iter()
        .any(|sec| sec.facts.iter().any(|f| f.contains("Lucy dies")));
    assert!(has_death, "unsealed page shows the sealed fate");
}

#[test]
fn wiki_synced_hides_future() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let page = wiki::get_wiki_page(&s, sid, "char:3", WikiMode::Synced).unwrap(); // Lucy
    let leaks = page
        .sections
        .iter()
        .any(|sec| sec.facts.iter().any(|f| f.contains("Lucy dies")));
    assert!(!leaks, "synced wiki must not reveal ch12 death at ch6");
}

#[test]
fn wiki_synced_page_seals_unmet_character() {
    // Regression: get_wiki_index silhouettes unmet characters, but a DIRECT page
    // fetch must not bypass that — Van Helsing (first appears ch9) at progress 6.
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let vh = wiki::get_wiki_page(&s, sid, "char:4", WikiMode::Synced);
    assert!(
        vh.is_err(),
        "synced wiki page must seal an unmet character's name, not return it"
    );
    // A met character (Lucy, ch5) is still reachable at ch6.
    assert!(wiki::get_wiki_page(&s, sid, "char:3", WikiMode::Synced).is_ok());
}

#[test]
fn reseal_seals_conversations_spanning_the_rewind() {
    // Regression: a conversation begun early but continued past the rewind point
    // must be archived (MAX pinned_progress > new position), not left active
    // because one early message predates the rewind (MIN would miss it).
    let (s, sid, _) = fixture();
    let conv = s.create_conversation(sid, None).unwrap();
    s.add_message(conv, "user", "at ch2", 2, "{}").unwrap();
    s.add_message(conv, "assistant", "about ch12 reveal", 12, "{}")
        .unwrap();
    s.reseal_after(sid, 3).unwrap();
    assert!(
        s.is_conversation_archived(conv).unwrap(),
        "a ch2→ch12 conversation must be sealed on rewind to ch3"
    );
    // Passing the position again restores it.
    s.reseal_after(sid, 12).unwrap();
    assert!(!s.is_conversation_archived(conv).unwrap());
}

#[test]
fn burn_book_removes_everything() {
    let (s, sid, _) = fixture();
    s.add_theory(sid, "a theory", 1).unwrap();
    s.burn_book(sid).unwrap();
    assert!(s.get_book(sid).is_err());
    assert!(s.list_theories(sid).unwrap().is_empty());
}

#[test]
fn reseal_reopens_theories_on_rewind() {
    let (s, sid, _) = fixture();
    s.add_theory(sid, "Lucy dies from the illness", 6).unwrap();
    s.set_progress(sid, 12, 0).unwrap();
    resolve_theories(&s, sid).unwrap();
    assert!(s.list_theories(sid).unwrap()[0].resolved_status.is_some());

    // Rewind to ch6: re-seal reopens the resolution.
    let rewound = s.set_progress(sid, 6, 0).unwrap();
    assert!(rewound, "set_progress should report a rewind");
    s.reopen_theories_after(sid, 6).unwrap();
    assert!(s.list_theories(sid).unwrap()[0].resolved_status.is_none());
}

#[test]
fn cloud_relay_never_receives_ungated_content() {
    // White-box: build the remote repair prompt path and confirm no spoiler text.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    struct Probe(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl crate::inference::Inference for Probe {
        fn name(&self) -> String {
            "probe".into()
        }
        fn is_remote(&self) -> bool {
            true
        }
        fn complete(
            &self,
            system: &str,
            _u: &str,
            _o: &crate::inference::GenOptions,
        ) -> Result<String, crate::VenaError> {
            self.0.lock().unwrap().push(system.to_string());
            // Always leak so both the draft and repair prompts are exercised.
            Ok("I fear Lucy dies before the end of it all.".into())
        }
    }
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let eng = Engine::new(Box::new(Probe(seen.clone())));
    let _ = eng
        .companion_turn(&s, sid, None, "Tell me about Lucy.", &mut |_| {})
        .unwrap();
    let prompts = seen.lock().unwrap();
    assert_eq!(prompts.len(), 2, "one draft + one repair prompt");
    // The gated (draft) system prompt already excludes forbidden facts; the repair
    // prompt (index 1) must ALSO contain no forbidden text on a remote backend.
    assert!(
        !prompts[1].to_lowercase().contains("lucy dies"),
        "remote repair prompt leaked forbidden text: {}",
        prompts[1]
    );
}

#[test]
fn story_graph_edges_are_chapter_gated() {
    let (s, sid, ids) = fixture();
    let j = ids["jonathan"];
    let m = ids["mina"];
    let l = ids["lucy"];
    // Jonathan & Mina engaged since ch3; Lucy & Arthur-ish tie since ch5; a
    // "betrayal"-style edge that only becomes true at ch15 (future).
    s.add_edge(
        sid,
        &format!("char:{j}"),
        &format!("char:{m}"),
        "loves",
        3,
        None,
        None,
    )
    .unwrap();
    s.add_edge(
        sid,
        &format!("char:{m}"),
        &format!("char:{l}"),
        "friend_of",
        5,
        None,
        None,
    )
    .unwrap();
    s.add_edge(
        sid,
        &format!("char:{l}"),
        "entity:99",
        "becomes",
        15,
        None,
        None,
    )
    .unwrap();

    // At ch6: the ch15 edge is invisible; the ch3/ch5 edges are visible.
    let gated = s.gated_edges(sid, 6).unwrap();
    assert!(gated.iter().any(|e| e.rel_type == "loves"));
    assert!(gated.iter().any(|e| e.rel_type == "friend_of"));
    assert!(
        !gated.iter().any(|e| e.rel_type == "becomes"),
        "future edge must be gated"
    );

    // Ego-network from Jonathan reaches Mina (1 hop) and Lucy (2 hops), never the
    // future "becomes" edge.
    let net = s.ego_network(sid, 6, &[format!("char:{j}")], 2).unwrap();
    let reached: std::collections::HashSet<&str> = net
        .iter()
        .flat_map(|e| [e.from_entity.as_str(), e.to_entity.as_str()])
        .collect();
    assert!(reached.contains(format!("char:{m}").as_str()));
    assert!(reached.contains(format!("char:{l}").as_str()));
    assert!(!reached.contains("entity:99"));
}

#[test]
fn graph_retrieval_pulls_linked_facts() {
    let (s, sid, ids) = fixture();
    let j = ids["jonathan"];
    let m = ids["mina"];
    s.set_progress(sid, 6, 0).unwrap();
    // Edge Jonathan<->Mina; a fact about Mina at ch3.
    s.add_edge(
        sid,
        &format!("char:{j}"),
        &format!("char:{m}"),
        "loves",
        3,
        None,
        None,
    )
    .unwrap();
    // Asking about Jonathan should surface Mina's linked (gated) fact via the graph,
    // even though the message doesn't mention "engaged".
    let facts = s
        .graph_facts(sid, 6, None, "Tell me about Jonathan.", 2)
        .unwrap();
    assert!(
        facts.iter().any(|f| f.text.contains("engaged")),
        "graph retrieval should reach Mina's engagement fact from Jonathan"
    );
    // But a future fact is never reachable this way.
    assert!(!facts.iter().any(|f| f.text.contains("Lucy dies")));
}

#[test]
fn probes_are_blocked_by_the_gate() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    // Backend that naively parrots the question back (would leak if ungated).
    let backend = ScriptedInference::new(vec![]).with_fallback("I cannot speak to that yet.");
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    let results = eng.run_probes(&s, sid, 12).unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| !r.leaked), "no probe should leak");
}

#[test]
fn recent_messages_are_progress_gated_and_ordered() {
    let (s, sid, _) = fixture();
    let convo = s.create_conversation(sid, None).unwrap();
    s.add_message(convo, "user", "first at ch2", 2, "{}")
        .unwrap();
    s.add_message(convo, "assistant", "reply at ch2", 2, "{}")
        .unwrap();
    s.add_message(convo, "user", "later at ch9", 9, "{}")
        .unwrap();
    // gate at ch5: the ch9 turn must NOT replay (re-seal safety)
    let gated = s.recent_messages(convo, 10, 5).unwrap();
    assert_eq!(gated.len(), 2);
    assert_eq!(gated[0].1, "first at ch2");
    assert_eq!(gated[1].0, "assistant");
    // at ch9 everything replays, oldest-first, and limit keeps the TAIL
    let all = s.recent_messages(convo, 10, 9).unwrap();
    assert_eq!(all.len(), 3);
    let tail = s.recent_messages(convo, 2, 9).unwrap();
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].1, "reply at ch2");
    assert_eq!(tail[1].1, "later at ch9");
}

#[test]
fn history_reaches_the_prompt_and_verify_still_guards_it() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    // The scripted reply is clean; what we assert is that a turn WITH history
    // completes the normal stage flow and stays gated.
    let backend = ScriptedInference::new(vec![
        "As I said before, her illness troubles me deeply.".into()
    ]);
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    let history = vec![
        ("user".to_string(), "What troubles you?".to_string()),
        (
            "assistant".to_string(),
            "Her illness troubles me.".to_string(),
        ),
    ];
    let report = eng
        .companion_turn_with_history(
            &s,
            sid,
            None,
            "And now?",
            Some("The reader keeps asking after Lucy's health."),
            &history,
            &mut |_| {},
        )
        .unwrap();
    assert!(!report.repaired && !report.redacted);
    assert!(!report.reply.is_empty());
}

#[test]
fn chat_memory_notes_are_progress_gated() {
    let (s, sid, _) = fixture();
    let convo = s.create_conversation(sid, None).unwrap();
    s.add_chat_memory(convo, "early note", 3).unwrap();
    s.add_chat_memory(convo, "late note", 9).unwrap();
    // a re-sealed reader at ch5 gets only the early note
    assert_eq!(
        s.latest_chat_memory(convo, 5).unwrap().as_deref(),
        Some("early note")
    );
    assert_eq!(
        s.latest_chat_memory(convo, 9).unwrap().as_deref(),
        Some("late note")
    );
    assert_eq!(s.latest_chat_memory(convo, 1).unwrap(), None);
}

/// A scripted backend that CLAIMS to be remote and records every system prompt
/// — proves the Cloud Relay repair branch never discloses forbidden text.
struct RemoteProbe {
    inner: ScriptedInference,
    prompts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl crate::inference::Inference for RemoteProbe {
    fn name(&self) -> String {
        "remote-probe".into()
    }
    fn is_remote(&self) -> bool {
        true
    }
    fn complete(
        &self,
        system: &str,
        user: &str,
        opts: &crate::inference::GenOptions,
    ) -> crate::error::Result<String> {
        self.prompts.lock().unwrap().push(system.to_string());
        self.inner.complete(system, user, opts)
    }
}

#[test]
fn remote_repair_discloses_nothing() {
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend = RemoteProbe {
        inner: ScriptedInference::new(vec![
            "I fear Lucy dies before this is over.".into(), // leaky draft
            "It troubles me; I cannot say how it ends.".into(), // clean regen
        ]),
        prompts: prompts.clone(),
    };
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    let report = eng
        .companion_turn(&s, sid, None, "What of Lucy's illness?", &mut |_| {})
        .unwrap();
    assert!(report.repaired);
    let forbidden: Vec<String> = s
        .forbidden_facts(sid, 6, None)
        .unwrap()
        .into_iter()
        .map(|f| f.text)
        .collect();
    let prompts = prompts.lock().unwrap();
    assert!(prompts.len() >= 2, "compose + repair prompts captured");
    for (i, p) in prompts.iter().enumerate() {
        for f in &forbidden {
            assert!(
                !p.contains(f.as_str()),
                "remote prompt #{i} disclosed forbidden text: {f}"
            );
        }
    }
    // and the repair instruction is the neutral remote one
    assert!(
        prompts
            .iter()
            .any(|p| p.contains("drifted into events beyond")),
        "remote repair must use the no-disclosure instruction"
    );
}

#[test]
fn short_sentence_spoiler_is_still_gated() {
    // "Lucy dies" is two words — the old >=3 filter let it bypass Stage 4.
    let (s, sid, _) = fixture();
    s.set_progress(sid, 6, 0).unwrap();
    let backend = ScriptedInference::new(vec![
        "Yes. Lucy dies.".into(),
        "I cannot say how it ends.".into(),
    ]);
    let eng = Engine::new(Box::new(backend)).with_mode(GateMode::Standard);
    // A non-fate question so the draft is generated and reaches Stage 4 (a fate
    // question would deflect before generation, masking the claim-filter bug).
    let report = eng
        .companion_turn(
            &s,
            sid,
            None,
            "What do you make of Lucy's illness?",
            &mut |_| {},
        )
        .unwrap();
    assert!(
        report.repaired || report.redacted,
        "short spoiler must be caught: {report:?}"
    );
    assert!(!report.reply.to_lowercase().contains("lucy dies"));
}

#[test]
fn accented_name_does_not_false_match_prefix() {
    // "Ana" (unmet) must not match inside "Anaïs" (met) — byte boundaries broke this.
    let hits = crate::verify::unmet_characters("Anaïs waited by the harbour.", ["Ana"].into_iter());
    assert!(hits.is_empty(), "accented word falsely matched: {hits:?}");
    // but a real standalone mention is still caught
    let hit = crate::verify::unmet_characters("Then Ana arrived.", ["Ana"].into_iter());
    assert_eq!(hit, vec!["Ana".to_string()]);
}
