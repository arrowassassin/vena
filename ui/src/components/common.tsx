// Shared house-style widgets: striped progress, stamps, panels, book cover.

import { BookMeta } from "../api";

export function Progress({ pct, cyan, animate }: { pct: number; cyan?: boolean; animate?: boolean }) {
  return (
    <div className={`v-progress ${cyan ? "cyan" : ""} ${animate ? "animate" : ""}`}>
      <span style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} />
    </div>
  );
}

export function Stamp({ children, red, slam }: { children: React.ReactNode; red?: boolean; slam?: boolean }) {
  return (
    <span className={`v-stamp ${red ? "v-stamp-red" : "v-stamp-cyan"} ${slam ? "v-stamp-slam" : ""}`}>
      {children}
    </span>
  );
}

export function MetaRow({ children }: { children: React.ReactNode }) {
  return <div className="v-meta flex flex-wrap items-center gap-x-2 gap-y-0.5">{children}</div>;
}

/// Typographic cover (generated placeholder is honest: composed from title/author,
/// spoiler-weight-0 info only — no fates, no twist imagery).
export function Cover({ book, className }: { book: BookMeta; className?: string }) {
  const hue = [...book.slug].reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
  return (
    <div
      className={`v-keyline relative overflow-hidden flex flex-col justify-between p-3 ${className ?? ""}`}
      style={{
        background: `linear-gradient(160deg, color-mix(in srgb, var(--panel) 82%, hsl(${hue} 60% 50%)), var(--panel))`,
      }}
    >
      {book.cover ? (
        <img src={book.cover} alt="" className="absolute inset-0 w-full h-full object-cover" />
      ) : (
        <>
          <div className="v-headline text-xl leading-none break-words">{book.title}</div>
          <div>
            <div className="v-meta">{(book.author ?? "").toUpperCase()}</div>
            <div className="h-1.5 mt-2" style={{ background: "var(--red)" }} />
          </div>
        </>
      )}
    </div>
  );
}

export function ForgeBadge({ book }: { book: BookMeta }) {
  if (book.forge_state === "sealed")
    return <span className="v-meta text-(--cyan) font-semibold">LEDGER SEALED ✓ · {book.fact_count} FACTS · {Math.round(book.ledger_coverage * 100)}%</span>;
  if (book.forge_state === "forging")
    return <span className="v-meta text-(--red) v-shimmer font-semibold">THE LEDGER IS STILL FORGING…</span>;
  return <span className="v-meta">UNFORGED — COMPANION NEEDS A LEDGER</span>;
}

export function EmptyState({ title, hint, action }: { title: string; hint: string; action?: React.ReactNode }) {
  return (
    <div className="max-w-md mx-auto mt-20 v-panel-shadow p-8 text-center v-fade">
      <div className="v-headline text-3xl mb-3">{title}</div>
      <p className="f-serif text-(--mut) mb-5">{hint}</p>
      {action}
    </div>
  );
}
