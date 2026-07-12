// LIBRARY — "YOUR SHELF". Book cards (cover, striped progress, forge status), the
// DROP AN EPUB import affordance with the WHAT FORGING DOES explainer, storage/
// privacy footer line, GET BOOKS entry (mobile's path to the Store), empty state.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { onEvent } from "../api";
import { useApp } from "../store";
import { Cover, EmptyState, ForgeBadge, MetaRow, Progress } from "../components/common";

export function Library() {
  const { books, refreshBooks, openBook, nav, showToast } = useApp();
  const [forging, setForging] = useState<{ pct: number; stage: string } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);

  useEffect(() => {
    return onEvent((e) => {
      if (e.name === "forge:progress") {
        setForging({ pct: Number(e.payload.pct), stage: String(e.payload.stage) });
      }
      if (e.name === "forge:done") {
        setForging(null);
        refreshBooks();
        showToast("LEDGER SEALED ✓");
      }
    });
  }, [refreshBooks, showToast]);

  const importPath = useCallback(
    async (path: string) => {
      setForging({ pct: 5, stage: "parse" });
      try {
        await api.importBook(path);
        await refreshBooks();
        showToast("FORGED ✓ — ON YOUR SHELF");
      } catch (e) {
        showToast(String((e as Error).message).toUpperCase().slice(0, 60));
      } finally {
        setForging(null);
      }
    },
    [refreshBooks, showToast],
  );

  // In Tauri the file dialog gives a real path; in the browser we accept a typed path
  // (the devserver reads the file server-side — same real pipeline).
  const browse = useCallback(async () => {
    const path = window.prompt("Path to .epub / .txt on this machine:");
    if (path) importPath(path);
  }, [importPath]);

  return (
    <div className="p-4 lg:p-8 max-w-6xl mx-auto">
      <div className="flex items-end justify-between mb-6">
        <div>
          <h1 className="v-headline text-4xl lg:text-5xl">YOUR SHELF</h1>
          <MetaRow>
            <span>{books.length} BOOKS</span>
            <span>·</span>
            <span className="text-(--cyan) font-semibold">100% LOCAL</span>
          </MetaRow>
        </div>
        <button className="v-btn v-btn-red hidden lg:block" onClick={() => nav("store")}>
          THE STORE →
        </button>
        <button className="v-btn v-btn-red lg:hidden" onClick={() => nav("store")}>
          GET BOOKS
        </button>
      </div>

      {books.length === 0 && !forging && (
        <EmptyState
          title="THE SHELF IS EMPTY"
          hint="Drop an EPUB below, or fetch a free classic from the Store — the ledger forges itself."
          action={
            <button className="v-btn v-btn-red" onClick={() => nav("store")}>
              GO TO THE STORE →
            </button>
          }
        />
      )}

      {/* shelf grid */}
      <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-4 gap-5">
        {books.map((b) => (
          <div key={b.id} className="v-panel-shadow p-3 flex flex-col gap-2 v-fade">
            <button onClick={() => { openBook(b); nav("reader"); }} className="text-left">
              <Cover book={b} className="aspect-2/3 w-full" />
            </button>
            <div className="f-cond text-sm leading-tight">{b.title}</div>
            <MetaRow>
              <span>{(b.author ?? "UNKNOWN").toUpperCase()}</span>
              <span>·</span>
              <span>{b.license.toUpperCase()}</span>
            </MetaRow>
            <Progress pct={(b.progress_episode / Math.max(1, b.episode_count)) * 100} />
            <MetaRow>
              <span>CH.{b.progress_episode}/{b.episode_count}</span>
              {b.profile !== "prose" && (
                <>
                  <span>·</span>
                  <span className="text-(--red)">{b.profile.toUpperCase()}</span>
                </>
              )}
            </MetaRow>
            <ForgeBadge book={b} />
            <div className="flex gap-2 mt-1">
              <button
                className="v-btn text-xs flex-1"
                onClick={() => { openBook(b); nav("reader"); }}
              >
                {b.progress_episode > 0 ? "CONTINUE READING →" : "BEGIN CHAPTER I →"}
              </button>
              <button
                className="v-btn v-btn-cyan text-xs"
                onClick={() => { openBook(b); nav("companion"); }}
              >
                COMPANION →
              </button>
            </div>
          </div>
        ))}
      </div>

      {/* forging progress */}
      {forging && (
        <div className="v-panel-shadow p-4 mt-6 v-fade">
          <div className="f-cond text-sm mb-2 v-shimmer">
            {forging.stage === "parse" && "PARSING THE BOOK…"}
            {forging.stage === "extract" && "EXTRACTING EVERY FACT…"}
            {forging.stage === "seal" && "SEALING THE LEDGER…"}
            {forging.stage === "done" && "DONE"}
          </div>
          <Progress pct={forging.pct} animate />
        </div>
      )}

      {/* import zone */}
      <div
        className={`mt-8 v-panel p-6 text-center border-dashed! transition-colors ${dragOver ? "bg-(--hatch)" : ""}`}
        onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          // Browsers cannot expose a filesystem path; in Tauri the drop carries one.
          const f = e.dataTransfer.files[0];
          if (f && "path" in f) importPath((f as File & { path: string }).path);
          else showToast("USE BROWSE FILES — DROPS NEED THE DESKTOP APP");
        }}
      >
        <div className="v-headline text-2xl mb-1">DROP AN EPUB</div>
        <MetaRow>
          <span className="mx-auto">.EPUB · .TXT · DRM-FREE · ~2 MIN TO FORGE</span>
        </MetaRow>
        <button className="v-btn mt-3" onClick={browse}>BROWSE FILES</button>
        <input ref={fileRef} type="file" accept=".epub,.txt" className="hidden" />

        <div className="grid md:grid-cols-3 gap-3 mt-6 text-left">
          {[
            ["1", "PARSE THE BOOK", "Chapters, scenes, who appears where."],
            ["2", "EXTRACT EVERY FACT", "Each one stamped with the chapter it becomes true."],
            ["3", "SEAL THE LEDGER", "The AI can only draw facts at or before your bookmark."],
          ].map(([n, t, d]) => (
            <div key={n} className="v-keyline p-3">
              <div className="f-cond text-xs text-(--red) mb-1">{n} · {t}</div>
              <div className="f-serif text-sm text-(--mut)">{d}</div>
            </div>
          ))}
        </div>
        <div className="v-meta mt-4">WHAT FORGING DOES · NOTHING LEAVES THIS DEVICE</div>
      </div>
    </div>
  );
}
