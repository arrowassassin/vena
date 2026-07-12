// READER — canon is sacred: pristine serif column, all AI affordances in margins/
// overlays. Typography controls (SIZE/FACE/LINE/WIDTH/ALIGN), paginated + scroll
// modes, chapter nav, MARK READ (moves the horizon), selection menu (ask the
// companion / dictionary hook), "pick a book" empty state, manual-position support.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, EpisodeHtml } from "../api";
import { useApp } from "../store";
import { EmptyState, MetaRow, Progress } from "../components/common";

type Face = "serif" | "sans";
type Align = "left" | "justify";

export function Reader() {
  const { book, books, openBook, nav, refreshBooks, showToast } = useApp();
  const [episode, setEpisode] = useState<EpisodeHtml | null>(null);
  const [seq, setSeq] = useState<number>(1);
  const [size, setSize] = useState(19);
  const [face, setFace] = useState<Face>("serif");
  const [lineHeight, setLineHeight] = useState(1.7);
  const [width, setWidth] = useState(68);
  const [align, setAlign] = useState<Align>("left");
  const [scrollMode, setScrollMode] = useState(true);
  const [page, setPage] = useState(0);
  const [showControls, setShowControls] = useState(false);
  const [selection, setSelection] = useState<{ text: string; x: number; y: number } | null>(null);
  const columnRef = useRef<HTMLDivElement>(null);

  // Open at the reader's bookmark (or ch.1).
  useEffect(() => {
    if (!book) return;
    const start = Math.max(1, Math.min(book.episode_count, book.progress_episode || 1));
    setSeq(start);
  }, [book]);

  useEffect(() => {
    if (!book) return;
    let live = true;
    api.getEpisode(book.id, seq).then((ep) => { if (live) { setEpisode(ep); setPage(0); } })
      .catch(() => setEpisode(null));
    return () => { live = false; };
  }, [book, seq]);

  const markRead = useCallback(async () => {
    if (!book) return;
    await api.setProgress(book.id, seq, 0);
    await refreshBooks();
    showToast("MARKING IT READ MOVES THE HORIZON");
  }, [book, seq, refreshBooks, showToast]);

  const jumpTo = useCallback(
    async (n: number) => {
      if (!book) return;
      if (n < (book.progress_episode || 0)) {
        showToast("JUMPING BACK RE-SEALS THE COMPANION TO THAT CHAPTER");
        await api.setProgress(book.id, n, 0);
        await refreshBooks();
      }
      setSeq(n);
    },
    [book, refreshBooks, showToast],
  );

  const onMouseUp = useCallback(() => {
    const sel = window.getSelection();
    const text = sel?.toString().trim() ?? "";
    if (text.length > 3 && sel && sel.rangeCount > 0) {
      const rect = sel.getRangeAt(0).getBoundingClientRect();
      setSelection({ text: text.slice(0, 400), x: rect.left + rect.width / 2, y: rect.top });
    } else {
      setSelection(null);
    }
  }, []);

  // Simple honest pagination: split rendered paragraphs into pages by count.
  const pages = useMemo(() => {
    if (!episode) return [] as string[][];
    const paras = episode.content_html.split("\n").filter(Boolean);
    const per = Math.max(4, Math.round(2600 / size / (lineHeight * 10)));
    const out: string[][] = [];
    for (let i = 0; i < paras.length; i += per) out.push(paras.slice(i, i + per));
    return out;
  }, [episode, size, lineHeight]);

  if (!book) {
    return (
      <EmptyState
        title="PICK A BOOK FROM YOUR SHELF"
        hint="The Reader needs a book context — choose one and the horizon follows your bookmark."
        action={
          <div className="flex flex-col gap-2">
            {books.slice(0, 4).map((b) => (
              <button key={b.id} className="v-btn" onClick={() => openBook(b)}>
                {b.title.toUpperCase()} →
              </button>
            ))}
            {books.length === 0 && (
              <button className="v-btn v-btn-red" onClick={() => nav("library")}>GO TO LIBRARY →</button>
            )}
          </div>
        }
      />
    );
  }

  const contentHtml = scrollMode
    ? episode?.content_html ?? ""
    : (pages[page] ?? []).join("\n");

  return (
    <div className="h-full flex flex-col" onMouseUp={onMouseUp}>
      {/* chapter bar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b-2 border-(--ink) bg-(--panel)">
        <button className="v-btn text-xs" onClick={() => jumpTo(Math.max(1, seq - 1))}>‹ PREV</button>
        <div className="flex-1 min-w-0">
          <div className="f-cond text-sm truncate">
            {episode?.title ?? `Chapter ${seq}`} <span className="v-meta">· {book.title.toUpperCase()}</span>
          </div>
          <Progress pct={(seq / Math.max(1, book.episode_count)) * 100} cyan />
        </div>
        <MetaRow>
          <span>{seq}/{book.episode_count}</span>
          {episode?.est_minutes && <span>· {episode.est_minutes} MIN</span>}
        </MetaRow>
        <button className="v-btn text-xs" onClick={() => jumpTo(Math.min(book.episode_count, seq + 1))}>NEXT ›</button>
        <button className="v-btn text-xs hidden md:block" onClick={() => setShowControls((v) => !v)}>Aa</button>
      </div>

      {/* typography controls */}
      {showControls && (
        <div className="flex flex-wrap gap-4 items-center px-4 py-2 border-b-2 border-(--ink) bg-(--panel) v-fade">
          <label className="v-meta flex items-center gap-2">SIZE
            <input type="range" min={14} max={28} value={size} onChange={(e) => setSize(+e.target.value)} />
          </label>
          <label className="v-meta flex items-center gap-2">FACE
            <button className={`v-btn text-xs ${face === "serif" ? "v-btn-ink" : ""}`} onClick={() => setFace("serif")}>SERIF</button>
            <button className={`v-btn text-xs ${face === "sans" ? "v-btn-ink" : ""}`} onClick={() => setFace("sans")}>SANS</button>
          </label>
          <label className="v-meta flex items-center gap-2">LINE
            <input type="range" min={13} max={22} value={lineHeight * 10} onChange={(e) => setLineHeight(+e.target.value / 10)} />
          </label>
          <label className="v-meta flex items-center gap-2">WIDTH
            <input type="range" min={48} max={90} value={width} onChange={(e) => setWidth(+e.target.value)} />
          </label>
          <label className="v-meta flex items-center gap-2">ALIGN
            <button className={`v-btn text-xs ${align === "left" ? "v-btn-ink" : ""}`} onClick={() => setAlign("left")}>LEFT</button>
            <button className={`v-btn text-xs ${align === "justify" ? "v-btn-ink" : ""}`} onClick={() => setAlign("justify")}>JUST</button>
          </label>
          <label className="v-meta flex items-center gap-2">PAGES
            <button className={`v-btn text-xs ${!scrollMode ? "v-btn-ink" : ""}`} onClick={() => setScrollMode(false)}>PAGED</button>
            <button className={`v-btn text-xs ${scrollMode ? "v-btn-ink" : ""}`} onClick={() => setScrollMode(true)}>SCROLL</button>
          </label>
        </div>
      )}

      {/* canon column — pristine */}
      <div ref={columnRef} className="flex-1 overflow-auto bg-(--paper)">
        <article
          className="v-canon mx-auto px-5 py-8 v-fade"
          style={{
            maxWidth: `${width}ch`,
            fontSize: `${size}px`,
            lineHeight,
            textAlign: align,
            fontFamily: face === "serif" ? undefined : "Oswald, sans-serif",
          }}
          dangerouslySetInnerHTML={{ __html: contentHtml }}
        />
        {!scrollMode && (
          <div className="flex justify-center items-center gap-4 pb-8">
            <button className="v-btn text-xs" disabled={page === 0} onClick={() => setPage((p) => p - 1)}>‹</button>
            <span className="v-meta">PAGE {page + 1}/{Math.max(1, pages.length)}</span>
            <button className="v-btn text-xs" disabled={page >= pages.length - 1} onClick={() => setPage((p) => p + 1)}>›</button>
          </div>
        )}
        {/* end-of-chapter horizon */}
        <div className="max-w-xl mx-auto px-5 pb-16">
          <div className="v-panel-shadow p-4 text-center">
            <div className="v-meta mb-2">THE HORIZON MOVES AS YOU READ</div>
            {book.progress_episode < seq ? (
              <button className="v-btn v-btn-red" onClick={markRead}>
                MARK CHAPTER {seq} READ →
              </button>
            ) : (
              <div className="f-cond text-sm text-(--cyan)">READ ✓ — THE LEDGER READ ALONG WITH YOU</div>
            )}
            <div className="mt-3 flex justify-center gap-2">
              <button className="v-btn text-xs" onClick={() => nav("companion")}>✦ ASK THE CAST</button>
              <button className="v-btn text-xs" onClick={() => nav("archive")}>THE ARCHIVE</button>
            </div>
          </div>
        </div>
      </div>

      {/* selection overlay — AI affordances live here, never inline */}
      {selection && (
        <div
          className="fixed z-50 v-panel-shadow p-1 flex gap-1 v-fade"
          style={{ left: Math.max(8, selection.x - 120), top: Math.max(8, selection.y - 46) }}
        >
          <button
            className="v-btn text-xs v-btn-cyan"
            onClick={() => {
              sessionStorage.setItem("vena.passage", selection.text);
              setSelection(null);
              nav("companion");
            }}
          >
            ✦ ASK THE COMPANION
          </button>
        </div>
      )}
    </div>
  );
}
