// App-wide state: current screen, active book, theme, settings. React context only —
// no localStorage (state persists through the Rust profile via typed commands).

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { api, BookMeta, Settings, AiStatus } from "./api";

export type Screen =
  | "library" | "store" | "companion" | "reader" | "archive" | "settings" | "onboarding";
export type Theme = "light" | "dark" | "sepia" | "oled";

interface AppState {
  screen: Screen;
  nav: (s: Screen) => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
  books: BookMeta[];
  refreshBooks: () => Promise<void>;
  book: BookMeta | null;             // active book context
  openBook: (b: BookMeta | null) => void;
  settings: Settings | null;
  refreshSettings: () => Promise<void>;
  ai: AiStatus | null;
  refreshAi: () => Promise<void>;
  toast: string | null;
  showToast: (msg: string) => void;
}

const Ctx = createContext<AppState | null>(null);

export function useApp(): AppState {
  const v = useContext(Ctx);
  if (!v) throw new Error("useApp outside provider");
  return v;
}

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [screen, setScreen] = useState<Screen>("library");
  const [theme, setThemeState] = useState<Theme>("light");
  const [books, setBooks] = useState<BookMeta[]>([]);
  const [book, setBook] = useState<BookMeta | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [ai, setAi] = useState<AiStatus | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const refreshBooks = useCallback(async () => {
    try {
      const list = await api.listBooks();
      setBooks(list);
      setBook((prev) => (prev ? list.find((b) => b.id === prev.id) ?? null : prev));
    } catch { /* devserver may still be starting */ }
  }, []);

  const refreshSettings = useCallback(async () => {
    try {
      const s = await api.getSettings();
      setSettings(s);
      const t = (s as unknown as Record<string, string>)["theme"] as Theme | undefined;
      if (t && ["light", "dark", "sepia", "oled"].includes(t)) setThemeState(t);
    } catch { /* not fatal */ }
  }, []);

  const refreshAi = useCallback(async () => {
    try { setAi(await api.getAiStatus()); } catch { /* not fatal */ }
  }, []);

  useEffect(() => {
    refreshBooks();
    refreshSettings();
    refreshAi();
  }, [refreshBooks, refreshSettings, refreshAi]);

  useEffect(() => {
    document.documentElement.setAttribute("data-vtheme", theme);
  }, [theme]);

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    api.setSetting("theme", t).catch(() => undefined); // persisted in the profile db
  }, []);

  const openBook = useCallback((b: BookMeta | null) => setBook(b), []);
  const nav = useCallback((s: Screen) => setScreen(s), []);
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 2600);
  }, []);

  const value = useMemo<AppState>(
    () => ({ screen, nav, theme, setTheme, books, refreshBooks, book, openBook, settings, refreshSettings, ai, refreshAi, toast, showToast }),
    [screen, nav, theme, setTheme, books, refreshBooks, book, openBook, settings, refreshSettings, ai, refreshAi, toast, showToast],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
