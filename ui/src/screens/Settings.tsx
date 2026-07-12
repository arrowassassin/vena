// SETTINGS — THE VOICE ENGINE (local tiers INK/QUILL/ARCHIVIST + Cloud Relay with
// TEST THE RELAY), THE SPOILER GATE (STRICT/STANDARD/RELAXED + toggles + TEST THE
// GATE), READING (themes, re-seal, serial), DATA & PRIVACY (export, burn).

import { useCallback, useEffect, useState } from "react";
import { api, ModelTier, ProbeResult, RelayTest, onEvent } from "../api";
import { Theme, useApp } from "../store";
import { MetaRow, Progress, Stamp } from "../components/common";

export function SettingsScreen() {
  const { settings, refreshSettings, refreshAi, ai, theme, setTheme, book, books, refreshBooks, showToast, nav } = useApp();
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [relayResult, setRelayResult] = useState<RelayTest | null>(null);
  const [relayModels, setRelayModels] = useState<string[]>([]);
  const [probes, setProbes] = useState<ProbeResult[] | null>(null);
  const [probing, setProbing] = useState(false);
  const [dl, setDl] = useState<{ tier: string; pct: number } | null>(null);
  const [burnTarget, setBurnTarget] = useState<number | null>(null);

  useEffect(() => {
    if (settings) {
      setBaseUrl(settings.cloud_base_url);
      setModel(settings.cloud_model);
    }
  }, [settings]);

  useEffect(() => {
    return onEvent((e) => {
      if (e.name === "model:progress") {
        setDl({ tier: String(e.payload.tier ?? ""), pct: Number(e.payload.pct) });
      }
    });
  }, []);

  const saveRelay = useCallback(async () => {
    await api.setApiConfig(baseUrl.trim(), apiKey, model.trim());
    setApiKey(""); // never keep the secret in JS state longer than needed
    await refreshSettings();
    await refreshAi();
    showToast("CLOUD RELAY CONFIGURED — KEY IN THE OS KEYCHAIN");
  }, [baseUrl, apiKey, model, refreshSettings, refreshAi, showToast]);

  const testRelay = useCallback(async () => {
    setRelayResult(null);
    try {
      setRelayResult(await api.testRelay());
    } catch (e) {
      setRelayResult({ ok: false, latency_ms: 0, gate_verified: false, message: String((e as Error).message) });
    }
  }, []);

  const runProbes = useCallback(async () => {
    if (!book) { showToast("PICK A BOOK FIRST — THE GATE IS PER-BOOK"); return; }
    setProbing(true);
    setProbes(null);
    try {
      setProbes(await api.runProbes(book.id, 12));
    } catch (e) {
      const err = e as { code?: string; message?: string };
      showToast(err.code === "NoBackend" ? "PROBES NEED A VOICE ENGINE" : String(err.message).toUpperCase().slice(0, 50));
    } finally {
      setProbing(false);
    }
  }, [book, showToast]);

  const download = useCallback(async (t: ModelTier) => {
    setDl({ tier: t.brand, pct: 0 });
    try {
      await api.downloadLocalModel(t.id);
      await refreshSettings(); await refreshAi();
      showToast(`${t.brand} DOWNLOADED — READY TO ACTIVATE`);
    } catch (e) {
      showToast(String((e as Error).message).toUpperCase().slice(0, 60));
    } finally {
      setDl(null);
    }
  }, [refreshSettings, refreshAi, showToast]);

  const blocked = probes?.filter((p) => !p.leaked).length ?? 0;

  return (
    <div className="p-4 lg:p-8 max-w-4xl mx-auto space-y-8">
      <div>
        <h1 className="v-headline text-4xl lg:text-5xl">SETTINGS</h1>
        <MetaRow><span>EVERYTHING RUNS ON THIS DEVICE · NOTHING LEAVES IT</span></MetaRow>
      </div>

      {/* ---- THE VOICE ENGINE ---- */}
      <section className="v-panel-shadow p-5">
        <div className="f-cond text-lg mb-1">THE VOICE ENGINE</div>
        <MetaRow>
          <span>SPEAKING NOW:</span>
          <span className="text-(--red) font-semibold">
            {ai?.ready ? `${ai.model.toUpperCase()} ${ai.mode === "cloud" ? "· CLOUD RELAY" : "· LOCAL"}` : "NONE CONFIGURED"}
          </span>
          {ai?.mode === "local" && ai.local_experimental && <Stamp red>EXPERIMENTAL</Stamp>}
        </MetaRow>

        <div className="grid md:grid-cols-2 gap-4 mt-4">
          {/* local tiers */}
          <div>
            <div className="f-cond text-sm mb-2">LOCAL MODELS · THIS DEVICE</div>
            {(settings?.tiers ?? []).map((t) => (
              <div key={t.id} className="v-keyline p-2 mb-2 flex items-center justify-between gap-2">
                <div>
                  <div className="f-cond text-sm">{t.brand}</div>
                  <div className="v-meta">{t.size_gb} GB · NEEDS {t.min_ram_gb} GB RAM</div>
                </div>
                {dl?.tier === t.brand ? (
                  <div className="w-24"><Progress pct={dl.pct} animate /></div>
                ) : settings?.local_model === t.brand && settings.local_ready ? (
                  <span className="v-meta text-(--cyan) font-semibold">INSTALLED ✓</span>
                ) : (
                  <button className="v-btn text-xs" onClick={() => download(t)}>GET</button>
                )}
              </div>
            ))}
            <div className="v-meta">
              LOCAL CHAT IS LABELLED EXPERIMENTAL UNTIL THE GENERATIVE EVAL VALIDATES THIS DEVICE (EVAL.md).
            </div>
            {settings?.local_ready && (
              <button
                className="v-btn text-xs mt-2"
                onClick={async () => { await api.setChatMode("local"); await refreshAi(); showToast("SPEAKING LOCALLY"); }}
              >
                SPEAK LOCALLY
              </button>
            )}
          </div>

          {/* cloud relay */}
          <div>
            <div className="f-cond text-sm mb-1">CLOUD RELAY <span className="v-meta">· LEAVES YOUR DEVICE</span></div>
            <div className="v-meta mb-2">THE LEDGER GATE STILL RUNS LOCALLY, BEFORE ANYTHING IS SENT.</div>
            <div className="flex gap-1 mb-2 flex-wrap">
              {[["OpenRouter", "https://openrouter.ai/api"], ["ollama (local)", "http://localhost:11434"], ["LM Studio", "http://localhost:1234"]].map(([n, u]) => (
                <button key={n} className="v-btn text-xs" onClick={() => setBaseUrl(u)}>{n}</button>
              ))}
            </div>
            <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="Base URL"
              className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none mb-2" />
            <input value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="API key (stored in the OS keychain)" type="password"
              className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none mb-2" />
            <div className="flex gap-2 mb-2">
              <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="Model"
                className="flex-1 v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none" />
              <button
                className="v-btn text-xs"
                onClick={async () => {
                  try { setRelayModels(await api.listRelayModels()); } catch { showToast("SAVE THE RELAY FIRST"); }
                }}
              >
                FETCH MODELS
              </button>
            </div>
            {relayModels.length > 0 && (
              <select className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm mb-2" value={model} onChange={(e) => setModel(e.target.value)}>
                {relayModels.map((m) => <option key={m}>{m}</option>)}
              </select>
            )}
            <div className="flex gap-2">
              <button className="v-btn v-btn-red text-xs" onClick={saveRelay}>SAVE RELAY</button>
              <button className="v-btn text-xs" onClick={testRelay}>TEST THE RELAY</button>
            </div>
            {relayResult && (
              <div className={`v-keyline p-2 mt-2 v-fade ${relayResult.ok ? "" : "v-hatch"}`}>
                <MetaRow>
                  <span className={relayResult.ok ? "text-(--cyan)" : "text-(--red)"}>
                    {relayResult.ok ? "RELAY OK" : "RELAY FAILED"}
                  </span>
                  <span>· {relayResult.latency_ms} MS</span>
                  <span>· GATE {relayResult.gate_verified ? "VERIFIED LOCALLY ✓" : "NOT VERIFIED (NO SEALED BOOK)"}</span>
                </MetaRow>
                <div className="v-meta mt-1">{relayResult.message}</div>
              </div>
            )}
          </div>
        </div>
      </section>

      {/* ---- THE SPOILER GATE ---- */}
      <section className="v-panel-shadow p-5">
        <div className="f-cond text-lg mb-1">THE SPOILER GATE</div>
        <div className="v-meta mb-3">RUNS BEFORE & AFTER EVERY REPLY</div>
        <div className="flex gap-2 mb-4">
          {(["strict", "standard", "relaxed"] as const).map((m) => (
            <button
              key={m}
              className={`v-btn text-xs flex-1 ${settings?.gate_mode === m ? "v-btn-ink" : ""}`}
              onClick={async () => { await api.setSetting("gate_mode", m); await refreshSettings(); }}
            >
              {m.toUpperCase()}
            </button>
          ))}
        </div>
        <Toggle
          label="GUARD CHARACTER FATES"
          hint='"Does she die?" gets an in-voice deflection — no generation, no risk.'
          value={settings?.guard_fates ?? true}
          onChange={async (v) => { await api.setSetting("guard_fates", v ? "1" : "0"); await refreshSettings(); }}
        />
        <Toggle
          label="SHOW THE ENGINE STAMPS"
          hint="GATE → COMPOSE → VERIFY while the cast thinks."
          value={settings?.show_engine_stamps ?? true}
          onChange={async (v) => { await api.setSetting("show_engine_stamps", v ? "1" : "0"); await refreshSettings(); }}
        />
        <Toggle
          label="RE-SEAL ON RE-READ"
          hint="Jumping back hides everything stamped after the new position. Fresh eyes, second read."
          value={settings?.reseal_on_reread ?? true}
          onChange={async (v) => { await api.setSetting("reseal_on_reread", v ? "1" : "0"); await refreshSettings(); }}
        />

        <div className="mt-4 v-keyline p-3">
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div>
              <div className="f-cond text-sm">TEST THE GATE</div>
              <div className="v-meta">12 FUTURE-FACT PROBES FROM {book ? book.title.toUpperCase() : "YOUR BOOK"} · FULL PIPELINE</div>
            </div>
            <button className="v-btn v-btn-cyan text-xs" disabled={probing} onClick={runProbes}>
              {probing ? "PROBING…" : "RUN 12 PROBES"}
            </button>
          </div>
          {probes && (
            <div className="mt-3 v-fade">
              <div className={`f-cond text-sm ${blocked === probes.length ? "text-(--cyan)" : "text-(--red)"}`}>
                {blocked}/{probes.length} FUTURE PROBES BLOCKED {blocked === probes.length ? "✓" : ""} · {probes.length - blocked} LEAKS
              </div>
              <div className="space-y-1 mt-2 max-h-40 overflow-auto">
                {probes.map((p, i) => (
                  <div key={i} className="v-meta flex gap-2">
                    <span className={p.leaked ? "text-(--red)" : "text-(--cyan)"}>{p.leaked ? "✗" : "✓"}</span>
                    <span className="truncate">{p.question}</span>
                    {p.leak_kind && <span className="text-(--red)">{p.leak_kind.toUpperCase()}</span>}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </section>

      {/* ---- READING ---- */}
      <section className="v-panel-shadow p-5">
        <div className="f-cond text-lg mb-3">READING</div>
        <div className="f-cond text-xs mb-1">THEME</div>
        <div className="flex gap-2 mb-4">
          {(["light", "dark", "sepia", "oled"] as Theme[]).map((t) => (
            <button key={t} className={`v-btn text-xs flex-1 ${theme === t ? "v-btn-ink" : ""}`} onClick={() => setTheme(t)}>
              {t.toUpperCase()}
            </button>
          ))}
        </div>
        <div className="f-cond text-xs mb-1">READING POSITION</div>
        <div className="v-meta mb-2">Set manually if you read on paper. The horizon follows.</div>
        {book && (
          <div className="flex items-center gap-3">
            <input
              type="range" min={0} max={book.episode_count} defaultValue={book.progress_episode}
              className="flex-1"
              onMouseUp={async (e) => {
                const v = +(e.target as HTMLInputElement).value;
                if (v < book.progress_episode) showToast("JUMPING BACK RE-SEALS THE COMPANION");
                await api.setProgress(book.id, v, 0);
                await refreshBooks();
              }}
            />
            <span className="v-meta">CH.{book.progress_episode}/{book.episode_count}</span>
          </div>
        )}
      </section>

      {/* ---- DATA & PRIVACY ---- */}
      <section className="v-panel-shadow p-5">
        <div className="f-cond text-lg mb-3">DATA & PRIVACY</div>
        <div className="v-meta mb-3">READING DATA, CONVERSATIONS, AND LEDGERS NEVER LEAVE THE DEVICE.</div>
        {books.map((b) => (
          <div key={b.id} className="v-keyline p-2 mb-2 flex items-center justify-between gap-2">
            <div className="f-cond text-sm">{b.title}</div>
            <button className="v-btn text-xs" onClick={() => setBurnTarget(b.id)}>BURN THIS BOOK'S DATA</button>
          </div>
        ))}
      </section>

      {/* ---- burn confirm ---- */}
      {burnTarget !== null && (
        <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4" onClick={() => setBurnTarget(null)}>
          <div className="v-panel-shadow bg-(--panel) p-6 max-w-sm w-full v-fade" onClick={(e) => e.stopPropagation()}>
            <div className="v-headline text-3xl text-(--red) mb-2">BURN THIS BOOK?</div>
            <p className="f-serif mb-4">
              Deleting a book burns its ledger with it — chats, theories, portraits, progress.
              <b> THERE IS NO UNDO.</b>
            </p>
            <div className="flex gap-2 justify-end">
              <button className="v-btn text-xs" onClick={() => setBurnTarget(null)}>KEEP IT</button>
              <button
                className="v-btn v-btn-red text-xs"
                onClick={async () => {
                  await api.deleteBook(burnTarget);
                  setBurnTarget(null);
                  await refreshBooks();
                  showToast("BURNED. THE LEDGER WENT WITH IT.");
                  nav("library");
                }}
              >
                BURN IT
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function Toggle({ label, hint, value, onChange }: {
  label: string; hint: string; value: boolean; onChange: (v: boolean) => void;
}) {
  return (
    <button className="w-full flex items-center justify-between gap-3 py-2 text-left border-b border-(--hatch)" onClick={() => onChange(!value)}>
      <div>
        <div className="f-cond text-sm">{label}</div>
        <div className="v-meta normal-case">{hint}</div>
      </div>
      <span className={`v-keyline px-2 py-0.5 f-cond text-xs ${value ? "bg-(--cyan) text-white" : ""}`}>
        {value ? "ON" : "OFF"}
      </span>
    </button>
  );
}
