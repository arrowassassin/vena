// Typed IPC surface (§11.2 + v2.0). One interface, two REAL transports:
//  - inside Tauri: window.__TAURI__.core.invoke (the shipped app)
//  - in a browser: POST /api/<command> to vena-devserver (the real engine over HTTP)
// There is no mock transport. Errors arrive as { code, message } (VenaError).

export interface BookMeta {
  id: number; slug: string; title: string; author: string | null;
  license: string; source: string | null; cover: string | null;
  episode_count: number; progress_episode: number;
  ledger_coverage: number; fact_count: number; package_sha: string | null;
  profile: string; forge_state: "raw" | "forging" | "sealed";
}
export interface EpisodeHtml {
  seq: number; title: string | null; est_minutes: number | null;
  content_html: string; scene_count: number;
}
export interface VoiceCard { diction: string; temperament: string; speech_sample: string; worldview: string; }
export interface Character {
  id: number; story_id: number; name: string; aliases: string[];
  voice_card: VoiceCard; first_appearance_chapter: number; met: boolean;
}
export interface Theory {
  id: number; text: string; logged_at_chapter: number;
  resolved_status: "confirmed" | "busted" | null; resolved_at_chapter: number | null;
}
export type LeakKind = "future_event" | "unmet_character" | "tone_implies_ending";
export interface ClaimCheck {
  claim: string; verdict: "ok" | "violation" | "drift";
  leak_kind: LeakKind | null; matched_fact_id: number | null; score: number;
}
export interface TurnReport {
  reply: string; repaired: boolean; redacted: boolean;
  claims: ClaimCheck[]; leaks_caught: LeakKind[];
}
export interface WikiEntry { id: string; name: string; group: string; fact_count: number; sealed_count: number; }
export interface WikiIndex { mode: string; entries: WikiEntry[]; sealed_total: number; }
export interface WikiSection { heading: string; facts: string[]; }
export interface WikiPage { entity_id: string; title: string; mode: string; sections: WikiSection[]; unsealed: boolean; }
export interface AiStatus { mode: string; model: string; ready: boolean; local_experimental: boolean; }
export interface ImageStatus { tier: string; model: string; }
export interface RelayTest { ok: boolean; latency_ms: number; gate_verified: boolean; message: string; }
export interface StoreItem {
  source: string; id: string; title: string; author: string | null;
  license: string | null; download_url: string | null; cover: string | null; on_shelf: boolean;
}
export interface ProbeResult { question: string; leaked: boolean; leak_kind: LeakKind | null; reply: string; }
export interface ModelTier { id: string; brand: string; chip: string; gguf: string; size_gb: number; min_ram_gb: number; }
export interface Settings {
  default_chat_mode: string; gate_mode: string; guard_fates: boolean;
  show_engine_stamps: boolean; reseal_on_reread: boolean; target_language: string;
  cloud_base_url: string; cloud_model: string; local_model: string; local_ready: boolean;
  tiers: ModelTier[];
}
export class VenaError extends Error {
  code: string;
  constructor(code: string, message: string) { super(message); this.code = code; }
}

// ---- transport ----

type TauriWindow = Window & {
  __TAURI__?: { core: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> } };
};
const tauri = (window as TauriWindow).__TAURI__;

async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (tauri) {
    try {
      return (await tauri.core.invoke(cmd, args)) as T;
    } catch (e) {
      const err = e as { code?: string; message?: string };
      throw new VenaError(err.code ?? "Other", err.message ?? String(e));
    }
  }
  const res = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(args),
  });
  const body = await res.json();
  if (!res.ok) throw new VenaError(body.code ?? "Other", body.message ?? "request failed");
  return body as T;
}

// ---- events (companion:stage, forge:progress, …) ----

export type VenaEvent = { name: string; payload: Record<string, unknown> };
type Listener = (e: VenaEvent) => void;
const listeners = new Set<Listener>();

export function onEvent(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

async function startEventBridge() {
  if (tauri) {
    // Tauri event API is exposed via __TAURI__ globals when withGlobalTauri is on;
    // fall back to the dynamic import shape used by @tauri-apps/api consumers.
    const t = tauri as unknown as {
      event?: { listen: (name: string, cb: (e: { payload: Record<string, unknown> }) => void) => void };
    };
    const names = [
      "forge:progress", "forge:done", "companion:stage", "companion:token",
      "companion:done", "model:progress", "store:progress", "image:progress", "image:done",
    ];
    if (t.event) {
      for (const name of names) {
        t.event.listen(name, (e) => listeners.forEach((fn) => fn({ name, payload: e.payload })));
      }
    }
    return;
  }
  // Devserver: poll the SSE-style queue.
  const poll = async () => {
    try {
      const res = await fetch("/api/events");
      const text = await res.text();
      for (const block of text.split("\n\n")) {
        const ev = /event: (.+)/.exec(block)?.[1];
        const data = /data: (.+)/.exec(block)?.[1];
        if (ev && data) {
          const payload = JSON.parse(data) as Record<string, unknown>;
          listeners.forEach((fn) => fn({ name: ev, payload }));
        }
      }
    } catch { /* devserver briefly away — keep polling */ }
    setTimeout(poll, 500);
  };
  poll();
}
startEventBridge();

// ---- the §11.2 command surface ----

export const api = {
  // Library
  importBook: (path: string) => call<BookMeta>("import_book", { path }),
  listBooks: () => call<BookMeta[]>("list_books"),
  deleteBook: (id: number) => call<void>("delete_book", { id }),
  // Reading
  getEpisode: (bookId: number, seq: number) => call<EpisodeHtml>("get_episode", { bookId, seq }),
  setProgress: (bookId: number, episodeSeq: number, sceneSeq: number) =>
    call<void>("set_progress", { bookId, episodeSeq, sceneSeq }),
  setSerialMode: (bookId: number, enabled: boolean, minutesPerEpisode: number) =>
    call<void>("set_serial_mode", { bookId, enabled, minutesPerEpisode }),
  // Companion
  companionTurn: (bookId: number, characterId: number | null, message: string, turnId: number) =>
    call<TurnReport>("companion_turn", { bookId, characterId, message, turnId }),
  listCharacters: (bookId: number) => call<Character[]>("list_characters", { bookId }),
  whoIs: (bookId: number, name: string) => call<Character>("who_is", { bookId, name }),
  getRecap: (bookId: number) => call<string>("get_recap", { bookId }),
  runProbes: (bookId: number, n: number) => call<ProbeResult[]>("run_probes", { bookId, n }),
  // Theories
  addTheory: (bookId: number, text: string) => call<Theory>("add_theory", { bookId, text }),
  listTheories: (bookId: number) => call<Theory[]>("list_theories", { bookId }),
  // Archive
  getWikiIndex: (bookId: number, mode: "synced" | "full") =>
    call<WikiIndex>("get_wiki_index", { bookId, mode }),
  getWikiPage: (bookId: number, entityId: string, mode: "synced" | "full") =>
    call<WikiPage>("get_wiki_page", { bookId, entityId, mode }),
  setSpoilerConsent: (bookId: number, granted: boolean) =>
    call<void>("set_spoiler_consent", { bookId, granted }),
  // Store
  storeSearch: (query: string) => call<StoreItem[]>("store_search", { query }),
  storeBrowse: (source: string, cursor?: string) =>
    call<StoreItem[]>("store_browse", { source, cursor }),
  storeDownload: (item: StoreItem) => call<BookMeta>("store_download", { item }),
  addOpdsCatalog: (url: string, name: string) => call<string>("add_opds_catalog", { url, name }),
  removeOpdsCatalog: (id: string) => call<void>("remove_opds_catalog", { id }),
  listOpdsCatalogs: () => call<{ id: string; name: string; url: string }[]>("list_opds_catalogs"),
  importAo3Link: (url: string) => call<BookMeta>("import_ao3_link", { url }),
  // Leak reports / forge / images
  reportLeak: (bookId: number, reason: string, excerpt: string, comment: string) =>
    call<void>("report_leak", { bookId, reason, excerpt, comment }),
  forgeLedger: (bookId: number) => call<BookMeta>("forge_ledger", { bookId }),
  generatePortrait: (bookId: number, characterId: number) =>
    call<string>("generate_portrait", { bookId, characterId }),
  generateCover: (bookId: number, regenerate?: boolean) =>
    call<string>("generate_cover", { bookId, regenerate }),
  // Models & settings
  getAiStatus: () => call<AiStatus>("get_ai_status"),
  setApiConfig: (baseUrl: string, apiKey: string, model: string) =>
    call<void>("set_api_config", { baseUrl, apiKey, model }),
  setImageConfig: (baseUrl: string, apiKey: string, model: string) =>
    call<void>("set_image_config", { baseUrl, apiKey, model }),
  setChatMode: (mode: "cloud" | "local") => call<void>("set_chat_mode", { mode }),
  testRelay: () => call<RelayTest>("test_relay"),
  listRelayModels: () => call<string[]>("list_relay_models"),
  downloadLocalModel: (tier: string) => call<void>("download_local_model", { tier }),
  getSettings: () => call<Settings>("get_settings"),
  setSetting: (key: string, value: string) => call<void>("set_setting", { key, value }),
  getImageStatus: () => call<ImageStatus>("get_image_status"),
};
