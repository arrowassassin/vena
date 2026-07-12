// THE ARCHIVE — the per-book wiki written by the ledger. SEALED (synced to your
// bookmark) by default; UNSEALED (full-spoiler) only behind the deliberate consent
// gate, with the persistent unsealed banner + RE-SEAL affordance. Fandom-style
// article layout; sealed entries show as redaction bars.

import { useCallback, useEffect, useState } from "react";
import { api, WikiIndex, WikiPage } from "../api";
import { useApp } from "../store";
import { EmptyState, MetaRow, Stamp } from "../components/common";

export function Archive() {
  const { book, books, openBook, showToast } = useApp();
  const [mode, setMode] = useState<"synced" | "full">("synced");
  const [index, setIndex] = useState<WikiIndex | null>(null);
  const [page, setPage] = useState<WikiPage | null>(null);
  const [consentGate, setConsentGate] = useState(false);
  const [understood, setUnderstood] = useState(false);

  const load = useCallback(async (m: "synced" | "full") => {
    if (!book) return;
    try {
      const idx = await api.getWikiIndex(book.id, m);
      setIndex(idx);
      setMode(m);
      setPage(null);
    } catch (e) {
      const err = e as { code?: string };
      if (err.code === "SpoilerConsentRequired") setConsentGate(true);
    }
  }, [book]);

  useEffect(() => { load("synced"); }, [load]);

  const openPage = useCallback(async (entityId: string) => {
    if (!book) return;
    try {
      setPage(await api.getWikiPage(book.id, entityId, mode));
    } catch { /* sealed page */ }
  }, [book, mode]);

  const unseal = useCallback(async () => {
    if (!book) return;
    await api.setSpoilerConsent(book.id, true);
    setConsentGate(false);
    setUnderstood(false);
    await load("full");
    showToast("UNSEALED. EVERYTHING IS ON THE TABLE.");
  }, [book, load, showToast]);

  const reseal = useCallback(async () => {
    if (!book) return;
    await api.setSpoilerConsent(book.id, false);
    await load("synced");
    showToast("RE-SEALED — SYNCED TO YOUR BOOKMARK");
  }, [book, load, showToast]);

  if (!book) {
    return (
      <EmptyState
        title="PICK A BOOK FROM YOUR SHELF"
        hint="Each book keeps its own archive, written by the ledger as you read."
        action={
          <div className="flex flex-col gap-2">
            {books.slice(0, 4).map((b) => (
              <button key={b.id} className="v-btn" onClick={() => openBook(b)}>{b.title.toUpperCase()} →</button>
            ))}
          </div>
        }
      />
    );
  }

  return (
    <div className="p-4 lg:p-8 max-w-5xl mx-auto">
      {/* unsealed banner — persistent and unmistakable */}
      {mode === "full" && (
        <div className="border-2 border-(--red) bg-(--red) text-white p-3 mb-4 flex items-center justify-between flex-wrap gap-2">
          <div className="f-cond text-sm">⚠ UNSEALED — THIS ARCHIVE SHOWS EVERYTHING. ENDINGS, DEATHS, TWISTS, THE LAST PAGE.</div>
          <button className="v-btn text-xs" onClick={reseal}>RE-SEAL IT</button>
        </div>
      )}

      <div className="flex items-end justify-between flex-wrap gap-2 mb-4">
        <div>
          <h1 className="v-headline text-4xl">{book.title.toUpperCase()} WIKI</h1>
          <MetaRow>
            <span>WRITTEN BY THE LEDGER · READ BY YOU</span>
            <span>·</span>
            <span>{index?.entries.length ?? 0} ENTRIES</span>
            {mode === "synced" && index && index.sealed_total > 0 && (
              <>
                <span>·</span>
                <span className="text-(--red)">{index.sealed_total} SEALED</span>
              </>
            )}
          </MetaRow>
        </div>
        {mode === "synced" ? (
          <button className="v-btn text-xs" onClick={() => setConsentGate(true)}>SPOILERS…</button>
        ) : null}
      </div>

      {book.fact_count === 0 ? (
        <div className="v-panel-shadow p-6 text-center">
          <div className="v-headline text-2xl mb-2">NOTHING TO UNSEAL YET</div>
          <div className="f-serif text-(--mut)">The archive opens the moment the ledger is sealed.</div>
        </div>
      ) : page ? (
        /* ---- article ---- */
        <div className="v-panel-shadow p-5 v-fade">
          <button className="v-meta underline mb-3" onClick={() => setPage(null)}>← ON THIS WIKI</button>
          <div className="flex items-center gap-3 mb-1">
            <h2 className="v-headline text-3xl">{page.title.toUpperCase()}</h2>
            {page.unsealed && <Stamp red>UNSEALED</Stamp>}
          </div>
          <MetaRow><span>ARTICLE</span><span>·</span><span>WRITTEN BY THE LEDGER — IT ACCEPTS NO HUMAN EDITS</span></MetaRow>
          <div className="mt-4 grid md:grid-cols-2 gap-4">
            {page.sections.map((s) => (
              <div key={s.heading} className="v-keyline p-3">
                <div className="f-cond text-sm text-(--red) mb-2">{s.heading.toUpperCase()}</div>
                <ul className="f-serif text-sm space-y-1.5">
                  {s.facts.map((f, i) => <li key={i}>{f}</li>)}
                </ul>
              </div>
            ))}
            {page.sections.length === 0 && (
              <div className="v-meta">EVERYTHING ABOUT THEM IS STILL SEALED — KEEP READING.</div>
            )}
          </div>
        </div>
      ) : (
        /* ---- index ---- */
        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {(["people", "places", "terms"] as const).map((group) => {
            const entries = index?.entries.filter((e) => e.group === group) ?? [];
            if (entries.length === 0) return null;
            return (
              <div key={group} className="v-panel-shadow p-3">
                <div className="f-cond text-sm mb-2">
                  {group === "people" ? "PEOPLE" : group === "places" ? "PLACES" : "TERMS & THINGS"}
                </div>
                <div className="space-y-1.5">
                  {entries.map((e) => (
                    <button
                      key={e.id}
                      onClick={() => openPage(e.id)}
                      className="w-full text-left v-keyline px-2 py-1.5 hover:bg-(--hatch) flex justify-between items-center"
                    >
                      <span className="f-serif">{e.name}</span>
                      <span className="v-meta">
                        {e.fact_count} FACTS{e.sealed_count > 0 && ` · ${e.sealed_count} SEALED`}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* ---- consent gate: deliberate, unmistakable ---- */}
      {consentGate && (
        <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4" onClick={() => setConsentGate(false)}>
          <div className="v-panel-shadow bg-(--panel) p-6 max-w-md w-full v-fade" onClick={(e) => e.stopPropagation()}>
            <div className="v-headline text-3xl text-(--red) mb-2">SPOILERS</div>
            <p className="f-serif mb-4">
              Unsealing shows <b>everything</b>. Endings, deaths, twists, the last page. There is
              no un-knowing it afterward.
            </p>
            <label className="flex items-start gap-2 f-cond text-xs mb-4 cursor-pointer">
              <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} className="mt-0.5" />
              I UNDERSTAND WHAT THIS ROOM IS
            </label>
            <div className="flex gap-2 justify-end">
              <button className="v-btn text-xs" onClick={() => setConsentGate(false)}>TAKE ME BACK</button>
              <button className="v-btn v-btn-red text-xs" disabled={!understood} onClick={unseal}>
                SPOIL ME — UNSEAL IT ALL
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
