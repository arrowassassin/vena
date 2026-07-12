// COMPANION — the heart. THE CAST SO FAR (unmet = silhouettes, "still ink"), chat
// with GATE→COMPOSE→VERIFY→(INKING OUT) engine stamps, the persistent "knows the
// story only up to Ch.N — like you" banner, shield icon on repaired replies,
// INKED OUT stamp on redactions, THAT SPOILED ME report, PREVIOUSLY ON… recap,
// THEORY BOARD (cards turn only when the bookmark passes the reveal), WHAT-IF.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, Character, Theory, TurnReport, onEvent } from "../api";
import { useApp } from "../store";
import { EmptyState, MetaRow, Stamp } from "../components/common";

type Msg = { role: "user" | "assistant"; text: string; report?: TurnReport };

export function Companion() {
  const { book, books, openBook, nav, settings, ai, showToast } = useApp();
  const [cast, setCast] = useState<Character[]>([]);
  const [active, setActive] = useState<Character | null>(null); // null = narrator
  const [messages, setMessages] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [stage, setStage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<"chat" | "recap" | "theories" | "whatif">("chat");
  const [recap, setRecap] = useState<string | null>(null);
  const [theories, setTheories] = useState<Theory[]>([]);
  const [theoryInput, setTheoryInput] = useState("");
  const [leakReport, setLeakReport] = useState<{ excerpt: string } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!book) return;
    api.listCharacters(book.id).then(setCast).catch(() => setCast([]));
    api.listTheories(book.id).then(setTheories).catch(() => setTheories([]));
  }, [book]);

  useEffect(() => {
    return onEvent((e) => {
      if (e.name === "companion:stage") setStage(String(e.payload.stage));
    });
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, stage]);

  // Passage handoff from the Reader ("ask the companion" deep-link).
  useEffect(() => {
    const passage = sessionStorage.getItem("vena.passage");
    if (passage) {
      sessionStorage.removeItem("vena.passage");
      setInput(`About this passage: “${passage.slice(0, 200)}…” — `);
    }
  }, []);

  const send = useCallback(async () => {
    if (!book || !input.trim() || busy) return;
    const message = input.trim();
    setInput("");
    setMessages((m) => [...m, { role: "user", text: message }]);
    setBusy(true);
    setStage("gate");
    try {
      const report = await api.companionTurn(book.id, active?.id ?? null, message, Date.now());
      setMessages((m) => [...m, { role: "assistant", text: report.reply, report }]);
    } catch (e) {
      const err = e as { code?: string; message?: string };
      if (err.code === "NoBackend") {
        showToast("NO VOICE ENGINE — CONFIGURE ONE IN SETTINGS");
        nav("settings");
      } else {
        showToast(String(err.message ?? e).toUpperCase().slice(0, 60));
      }
    } finally {
      setBusy(false);
      setStage(null);
    }
  }, [book, input, busy, active, nav, showToast]);

  const loadRecap = useCallback(async () => {
    if (!book) return;
    setRecap(null);
    setBusy(true);
    try {
      setRecap(await api.getRecap(book.id));
    } catch (e) {
      const err = e as { code?: string };
      setRecap(err.code === "NoBackend"
        ? "The narrator needs a voice engine — configure one in Settings."
        : "The narrator lost the thread — try again.");
    } finally {
      setBusy(false);
    }
  }, [book]);

  const addTheory = useCallback(async () => {
    if (!book || !theoryInput.trim()) return;
    const t = await api.addTheory(book.id, theoryInput.trim());
    setTheories((ts) => [...ts, t]);
    setTheoryInput("");
    showToast("PINNED TO THE BOARD");
  }, [book, theoryInput, showToast]);

  if (!book) {
    return (
      <EmptyState
        title="PICK A BOOK FROM YOUR SHELF"
        hint="The cast assembles per book — choose one and speak with them."
        action={
          <div className="flex flex-col gap-2">
            {books.slice(0, 4).map((b) => (
              <button key={b.id} className="v-btn" onClick={() => openBook(b)}>
                {b.title.toUpperCase()} →
              </button>
            ))}
          </div>
        }
      />
    );
  }

  const ch = book.progress_episode;
  const showStamps = settings?.show_engine_stamps ?? true;

  return (
    <div className="h-full flex flex-col lg:flex-row">
      {/* ---- cast rail ---- */}
      <aside className="lg:w-72 border-b-2 lg:border-b-0 lg:border-r-2 border-(--ink) bg-(--panel) overflow-auto">
        <div className="p-3">
          <div className="f-cond text-sm mb-1">THE CAST SO FAR</div>
          <MetaRow>
            <span>{cast.filter((c) => c.met).length} MET</span>
            <span>·</span>
            <span>{cast.filter((c) => !c.met).length} STILL INK</span>
          </MetaRow>
        </div>
        <div className="flex lg:flex-col gap-2 p-3 pt-0 overflow-x-auto">
          <button
            onClick={() => { setActive(null); setMessages([]); setTab("chat"); }}
            className={`v-keyline p-2 text-left shrink-0 lg:shrink ${active === null ? "bg-(--ink) text-(--inv)" : "bg-(--panel)"}`}
          >
            <div className="f-cond text-sm">THE NARRATOR</div>
            <div className="v-meta">SPOILER-SAFE GUIDE</div>
          </button>
          {cast.map((c) =>
            c.met ? (
              <button
                key={c.id}
                onClick={() => { setActive(c); setMessages([]); setTab("chat"); }}
                className={`v-keyline p-2 text-left shrink-0 lg:shrink ${active?.id === c.id ? "bg-(--ink) text-(--inv)" : "bg-(--panel)"}`}
              >
                <div className="f-cond text-sm">{c.name}</div>
                <div className="v-meta">SINCE CH.{c.first_appearance_chapter}</div>
              </button>
            ) : (
              <div key={c.id} className="v-keyline p-2 v-silhouette shrink-0 lg:shrink select-none" title="KEEP READING">
                <div className="f-cond text-sm">████ ██ █████</div>
                <div className="v-meta" style={{ color: "var(--silfg)" }}>UNMET · KEEP READING</div>
              </div>
            ),
          )}
        </div>
        {cast.length > 0 && cast.every((c) => !c.met) && (
          <div className="p-3 v-meta">THE WHOLE CAST IS STILL INK. THEY STEP OUT AS YOU MEET THEM.</div>
        )}
        {book.forge_state !== "sealed" && (
          <div className="p-3">
            <div className="v-meta text-(--red) v-shimmer">THE LEDGER IS STILL FORGING</div>
          </div>
        )}
      </aside>

      {/* ---- main pane ---- */}
      <section className="flex-1 flex flex-col min-h-0">
        {/* knowledge banner */}
        <div className="px-4 py-2 border-b-2 border-(--ink) bg-(--panel) flex items-center justify-between gap-2 flex-wrap">
          <div className="f-cond text-xs">
            {active ? active.name.toUpperCase() : "THE NARRATOR"} KNOWS THE STORY ONLY UP TO{" "}
            <span className="text-(--red)">CH.{ch}</span> — LIKE YOU. KNOWS NOTHING BEYOND IT.
          </div>
          <div className="flex gap-1">
            {(["chat", "recap", "theories", "whatif"] as const).map((t) => (
              <button
                key={t}
                onClick={() => { setTab(t); if (t === "recap" && recap === null) loadRecap(); }}
                className={`v-btn text-xs ${tab === t ? "v-btn-ink" : ""}`}
              >
                {t === "chat" ? "CHAT" : t === "recap" ? "PREVIOUSLY ON…" : t === "theories" ? "THEORY BOARD" : "WHAT-IF"}
              </button>
            ))}
          </div>
        </div>

        {tab === "chat" && (
          <>
            <div className="flex-1 overflow-auto p-4 space-y-3">
              {messages.length === 0 && (
                <div className="v-meta text-center mt-10">
                  {ai?.ready
                    ? `EVERY REPLY GATED ≤ CH.${ch} · VERIFIED AGAINST THE LEDGER`
                    : "NO VOICE ENGINE CONFIGURED — SET UP LOCAL OR CLOUD RELAY IN SETTINGS"}
                </div>
              )}
              {messages.map((m, i) => (
                <div key={i} className={`max-w-[76ch] v-fade ${m.role === "user" ? "ml-auto" : ""}`}>
                  <div
                    className={`p-3 border-2 border-(--ink) ${m.role === "user" ? "bg-(--ink) text-(--inv)" : ""}`}
                    style={m.role === "assistant" ? { background: "var(--bub)" } : undefined}
                  >
                    <div className="f-serif whitespace-pre-wrap">{m.text}</div>
                    {m.report && (m.report.repaired || m.report.redacted) && (
                      <div className="mt-2 flex items-center gap-2">
                        {m.report.redacted ? (
                          <Stamp red slam>INKED OUT!</Stamp>
                        ) : (
                          <span className="v-meta text-(--cyan)">🛡 REPAIRED FOR SPOILER-SAFETY</span>
                        )}
                        <button className="v-meta underline" onClick={() => setLeakReport({ excerpt: m.text })}>
                          ⚑ REPORT A LEAK
                        </button>
                      </div>
                    )}
                    {m.report && !m.report.repaired && !m.report.redacted && m.role === "assistant" && (
                      <div className="mt-1 flex justify-end">
                        <button className="v-meta underline opacity-60" onClick={() => setLeakReport({ excerpt: m.text })}>
                          THAT SPOILED ME
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              ))}
              {busy && showStamps && (
                <div className="flex gap-2 items-center v-fade">
                  {["gate", "compose", "verify", "repair"].map((s) => (
                    <span
                      key={s}
                      className={`v-stamp ${stage === s ? "v-stamp-red v-shimmer" : "opacity-30"}`}
                      style={{ transform: "rotate(-4deg)" }}
                    >
                      {s === "gate" ? "GATE" : s === "compose" ? "COMPOSE" : s === "verify" ? "VERIFY" : "INKING OUT"}
                    </span>
                  ))}
                </div>
              )}
              {busy && !showStamps && <div className="v-meta v-blink">THE CAST IS THINKING…</div>}
              <div ref={bottomRef} />
            </div>
            <div className="p-3 border-t-2 border-(--ink) bg-(--panel) flex gap-2">
              <input
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && send()}
                placeholder={active ? `Speak with ${active.name}…` : "Ask the narrator…"}
                className="flex-1 v-keyline bg-(--bub) px-3 py-2 f-serif outline-none"
              />
              <button className="v-btn v-btn-red" disabled={busy} onClick={send}>SEND!</button>
            </div>
            <div className="px-3 pb-2 bg-(--panel) v-meta">SPOILER-RESISTANT ≠ SPOILER-PROOF · EVERY REPLY GATED ≤ CH.{ch}</div>
          </>
        )}

        {tab === "recap" && (
          <div className="flex-1 overflow-auto p-6">
            <div className="v-panel-shadow p-5 max-w-2xl mx-auto">
              <div className="f-cond text-sm mb-3">PREVIOUSLY ON… <span className="text-(--red)">{book.title.toUpperCase()}</span></div>
              {recap === null ? (
                <div className="v-meta v-blink">THE NARRATOR CLEARS THEIR THROAT…</div>
              ) : (
                <div className="f-serif whitespace-pre-wrap">{recap}</div>
              )}
              <button className="v-btn text-xs mt-4" onClick={loadRecap} disabled={busy}>▶ RETELL IT</button>
            </div>
          </div>
        )}

        {tab === "theories" && (
          <div className="flex-1 overflow-auto p-6">
            <div className="max-w-2xl mx-auto">
              <div className="f-cond text-sm mb-1">THEORY BOARD</div>
              <div className="v-meta mb-4">CARDS TURN ONLY WHEN YOUR BOOKMARK PASSES THE REVEAL</div>
              <div className="flex gap-2 mb-4">
                <input
                  value={theoryInput}
                  onChange={(e) => setTheoryInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addTheory()}
                  placeholder="I think the Count is…"
                  className="flex-1 v-keyline bg-(--bub) px-3 py-2 f-serif outline-none"
                />
                <button className="v-btn v-btn-cyan" onClick={addTheory}>PIN IT</button>
              </div>
              <div className="grid sm:grid-cols-2 gap-3">
                {theories.map((t) => (
                  <div key={t.id} className={`v-panel-shadow p-3 ${t.resolved_status ? "v-hatch" : ""}`}>
                    <div className="f-serif mb-2">“{t.text}”</div>
                    <MetaRow>
                      <span>PINNED CH.{t.logged_at_chapter}</span>
                      <span>·</span>
                      {t.resolved_status ? (
                        <Stamp red={t.resolved_status === "busted"} slam>
                          {t.resolved_status === "confirmed" ? "CALLED IT!!" : "BUSTED!!"}
                        </Stamp>
                      ) : (
                        <span className="text-(--cyan)">OPEN</span>
                      )}
                      {t.resolved_at_chapter && <span>· RESOLVED CH.{t.resolved_at_chapter}</span>}
                    </MetaRow>
                  </div>
                ))}
                {theories.length === 0 && <div className="v-meta">NO THEORIES PINNED YET — CALL YOUR SHOT.</div>}
              </div>
            </div>
          </div>
        )}

        {tab === "whatif" && (
          <div className="flex-1 overflow-auto p-6">
            <div className="max-w-2xl mx-auto v-panel-shadow p-5 v-hatch">
              <div className="flex items-center gap-2 mb-2">
                <Stamp red>AI BRANCH · NOT CANON</Stamp>
                <span className="v-meta">THE BOOK ITSELF IS NEVER TOUCHED</span>
              </div>
              <div className="f-serif text-(--mut)">
                What-if branches fork from a chapter and are always labeled. Ask the narrator in
                CHAT: “What if Jonathan turned back at the Borgo Pass?” — the branch stays in the
                margins, clearly stamped, and canon remains pristine.
              </div>
            </div>
          </div>
        )}
      </section>

      {/* ---- leak report modal (THAT SPOILED ME) ---- */}
      {leakReport && (
        <LeakReportModal
          bookId={book.id}
          excerpt={leakReport.excerpt}
          onClose={() => setLeakReport(null)}
          onFiled={() => { setLeakReport(null); showToast("FILED. NOTHING IN THIS REPORT LEAVES YOUR DEVICE."); }}
        />
      )}
    </div>
  );
}

function LeakReportModal({ bookId, excerpt, onClose, onFiled }: {
  bookId: number; excerpt: string; onClose: () => void; onFiled: () => void;
}) {
  const [reason, setReason] = useState<string>("future");
  const [comment, setComment] = useState("");
  return (
    <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4" onClick={onClose}>
      <div className="v-panel-shadow bg-(--panel) p-5 max-w-md w-full v-fade" onClick={(e) => e.stopPropagation()}>
        <div className="v-headline text-2xl mb-1">THAT SPOILED ME</div>
        <div className="v-meta mb-3">CAPTURED CONTEXT · THE LINE:</div>
        <div className="v-keyline p-2 f-serif text-sm mb-3 max-h-24 overflow-auto">“{excerpt.slice(0, 240)}”</div>
        <div className="f-cond text-xs mb-1">WHAT LEAKED?</div>
        <div className="flex gap-2 mb-3 flex-wrap">
          {[["future", "A FUTURE EVENT"], ["character", "SOMEONE UNMET"], ["tone", "THE TONE SAID TOO MUCH"], ["other", "OTHER"]].map(([v, l]) => (
            <button key={v} className={`v-btn text-xs ${reason === v ? "v-btn-red" : ""}`} onClick={() => setReason(v)}>
              {l}
            </button>
          ))}
        </div>
        <textarea
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          placeholder="Anything else? (optional)"
          className="w-full v-keyline bg-(--bub) p-2 f-serif text-sm mb-3 outline-none"
          rows={2}
        />
        <div className="flex gap-2 justify-end">
          <button className="v-btn text-xs" onClick={onClose}>CANCEL</button>
          <button
            className="v-btn v-btn-red text-xs"
            onClick={async () => {
              await api.reportLeak(bookId, reason, excerpt.slice(0, 500), comment);
              onFiled();
            }}
          >
            FILE THE REPORT
          </button>
        </div>
        <div className="v-meta mt-2">NOTHING IN THIS REPORT LEAVES YOUR DEVICE.</div>
      </div>
    </div>
  );
}
