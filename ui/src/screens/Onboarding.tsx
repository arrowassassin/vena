// FIRST-RUN ONBOARDING — 3 steps max (design brief): 1 pick/keep a book →
// 2 choose the voice (LOCAL pre-selected vs CLOUD RELAY, per the v2.0 binary
// choice + the EVAL.md steer note) → 3 begin. Skippable at every step.

import { useState } from "react";
import { api } from "../api";
import { useApp } from "../store";
import { MetaRow } from "../components/common";

export function Onboarding() {
  const { books, nav, refreshAi, showToast } = useApp();
  const [step, setStep] = useState(1);
  const [choice, setChoice] = useState<"local" | "cloud">("cloud");
  const [baseUrl, setBaseUrl] = useState("https://openrouter.ai/api");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");

  return (
    <div className="h-full flex items-center justify-center p-4">
      <div className="v-panel-shadow bg-(--panel) p-6 lg:p-10 max-w-lg w-full v-fade">
        <div className="v-headline text-5xl mb-1">VENA</div>
        <MetaRow><span>THE SPOILER-SAFE READING COMPANION</span><span>· STEP {step}/3</span></MetaRow>

        {step === 1 && (
          <div className="mt-6">
            <div className="f-cond text-lg mb-2">A FRESH LEDGER</div>
            <p className="f-serif text-(--mut) mb-4">
              {books.length > 0
                ? `${books[0].title} is already on your shelf, forged and sealed. Bring your own books any time — nothing leaves this device.`
                : "Bring any book you own — the ledger forges itself, on this device."}
            </p>
            <button className="v-btn v-btn-red w-full" onClick={() => setStep(2)}>NEXT →</button>
          </div>
        )}

        {step === 2 && (
          <div className="mt-6">
            <div className="f-cond text-lg mb-2">CHOOSE THE VOICE</div>
            <div className="space-y-2 mb-4">
              <button
                className={`w-full v-keyline p-3 text-left ${choice === "cloud" ? "bg-(--ink) text-(--inv)" : ""}`}
                onClick={() => setChoice("cloud")}
              >
                <div className="f-cond text-sm">CLOUD RELAY · RECOMMENDED</div>
                <div className="v-meta">YOUR OWN KEY · THE LEDGER GATE STILL RUNS LOCALLY, BEFORE ANYTHING IS SENT</div>
              </button>
              <button
                className={`w-full v-keyline p-3 text-left ${choice === "local" ? "bg-(--ink) text-(--inv)" : ""}`}
                onClick={() => setChoice("local")}
              >
                <div className="f-cond text-sm">LOCAL · EXPERIMENTAL</div>
                <div className="v-meta">DOWNLOADS A MODEL TO THIS DEVICE · VALIDATE IT WITH TEST THE GATE</div>
              </button>
            </div>
            {choice === "cloud" && (
              <div className="space-y-2 mb-4">
                <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="Base URL"
                  className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none" />
                <input value={apiKey} onChange={(e) => setApiKey(e.target.value)} type="password" placeholder="API key (goes to the OS keychain)"
                  className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none" />
                <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="Model (e.g. anthropic/claude-haiku-4-5)"
                  className="w-full v-keyline bg-(--bub) px-2 py-1.5 f-mono text-sm outline-none" />
              </div>
            )}
            <div className="flex gap-2">
              <button className="v-btn text-xs" onClick={() => setStep(3)}>SKIP AI FOR NOW</button>
              <button
                className="v-btn v-btn-red flex-1"
                onClick={async () => {
                  if (choice === "cloud" && apiKey.trim()) {
                    await api.setApiConfig(baseUrl.trim(), apiKey.trim(), model.trim() || "openrouter/auto");
                    await refreshAi();
                    showToast("RELAY CONFIGURED");
                  } else if (choice === "local") {
                    showToast("PICK A TIER IN SETTINGS → THE VOICE ENGINE");
                  }
                  setStep(3);
                }}
              >
                NEXT →
              </button>
            </div>
          </div>
        )}

        {step === 3 && (
          <div className="mt-6">
            <div className="f-cond text-lg mb-2">THE HORIZON MOVES AS YOU READ</div>
            <p className="f-serif text-(--mut) mb-4">
              The cast never knows more of the story than you do. Read here, or set your position
              by hand if you read on paper — the companion follows your bookmark either way.
            </p>
            <button className="v-btn v-btn-red w-full" onClick={() => nav("library")}>
              BEGIN CHAPTER I →
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
