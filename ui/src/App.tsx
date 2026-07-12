// Shell: desktop = six-item top nav LIBRARY · STORE · COMPANION · READER · ARCHIVE ·
// SETTINGS; mobile (<1024px) = 5 tabs SHELF · CAST · READ · WIKI · SET, with the
// Store reached from Shelf's GET BOOKS (never a mobile tab) — §11.4a.

import { AppProvider, Screen, useApp } from "./store";
import { Library } from "./screens/Library";
import { StoreScreen } from "./screens/Store";
import { Companion } from "./screens/Companion";
import { Reader } from "./screens/Reader";
import { Archive } from "./screens/Archive";
import { SettingsScreen } from "./screens/Settings";
import { Onboarding } from "./screens/Onboarding";

const DESKTOP_NAV: { id: Screen; label: string }[] = [
  { id: "library", label: "LIBRARY" },
  { id: "store", label: "STORE" },
  { id: "companion", label: "COMPANION" },
  { id: "reader", label: "READER" },
  { id: "archive", label: "ARCHIVE" },
  { id: "settings", label: "SETTINGS" },
];

// Mobile 5-tab (normative): SHELF · CAST · READ · WIKI · SET. No Store tab.
const MOBILE_NAV: { id: Screen; label: string; glyph: string }[] = [
  { id: "library", label: "SHELF", glyph: "▤" },
  { id: "companion", label: "CAST", glyph: "◉" },
  { id: "reader", label: "READ", glyph: "❦" },
  { id: "archive", label: "WIKI", glyph: "◈" },
  { id: "settings", label: "SET", glyph: "⚙" },
];

function Shell() {
  const { screen, nav, toast, book } = useApp();

  const body = (() => {
    switch (screen) {
      case "library": return <Library />;
      case "store": return <StoreScreen />;
      case "companion": return <Companion />;
      case "reader": return <Reader />;
      case "archive": return <Archive />;
      case "settings": return <SettingsScreen />;
      case "onboarding": return <Onboarding />;
    }
  })();

  if (screen === "onboarding") return <div className="h-full v-dotgrid">{body}</div>;

  return (
    <div className="h-full flex flex-col v-dotgrid">
      {/* ---- desktop top nav ---- */}
      <header className="hidden lg:flex items-stretch border-b-2 border-(--ink) bg-(--panel) select-none">
        <div className="px-5 py-3 border-r-2 border-(--ink) flex items-center gap-3">
          <span className="v-headline text-2xl">VENA</span>
          <span className="v-meta hidden xl:inline">THE SPOILER-SAFE READING COMPANION</span>
        </div>
        <nav className="flex flex-1">
          {DESKTOP_NAV.map((n) => (
            <button
              key={n.id}
              onClick={() => nav(n.id)}
              className={`f-cond px-5 text-sm border-r-2 border-(--ink) transition-colors ${
                screen === n.id ? "bg-(--ink) text-(--inv)" : "hover:bg-(--hatch)"
              }`}
            >
              {n.label}
            </button>
          ))}
        </nav>
        {book && (
          <div className="v-meta flex items-center px-4 gap-2 border-l-2 border-(--ink)">
            <span className="text-(--red) font-semibold">{book.title.toUpperCase()}</span>
            <span>CH.{book.progress_episode}/{book.episode_count}</span>
          </div>
        )}
      </header>

      {/* ---- body ---- */}
      <main className="flex-1 overflow-auto pb-16 lg:pb-0">{body}</main>

      {/* ---- mobile 5-tab bar ---- */}
      <nav className="lg:hidden fixed bottom-0 inset-x-0 z-40 grid grid-cols-5 border-t-2 border-(--ink) bg-(--panel)">
        {MOBILE_NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => nav(n.id)}
            className={`f-cond text-[10px] py-2 flex flex-col items-center gap-0.5 ${
              screen === n.id ? "bg-(--ink) text-(--inv)" : ""
            }`}
          >
            <span className="text-base leading-none">{n.glyph}</span>
            {n.label}
          </button>
        ))}
      </nav>

      {toast && <div className="v-toast">{toast}</div>}
    </div>
  );
}

export function App() {
  return (
    <AppProvider>
      <Shell />
    </AppProvider>
  );
}
