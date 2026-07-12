// THE STORE — four acquisition paths, one protocol (§F4): FEATURED (vena-catalog),
// PROJECT GUTENBERG (Gutendex search-first), STANDARD EBOOKS (OPDS), YOUR CATALOGS
// (user OPDS), plus AO3 link-paste. Everything downloads to this device.

import { useCallback, useEffect, useState } from "react";
import { api, StoreItem, onEvent } from "../api";
import { useApp } from "../store";
import { MetaRow, Progress } from "../components/common";

export function StoreScreen() {
  const { refreshBooks, showToast, nav } = useApp();
  const [query, setQuery] = useState("");
  const [featured, setFeatured] = useState<StoreItem[]>([]);
  const [results, setResults] = useState<StoreItem[]>([]);
  const [seItems, setSeItems] = useState<StoreItem[]>([]);
  const [catalogs, setCatalogs] = useState<{ id: string; name: string; url: string }[]>([]);
  const [ao3, setAo3] = useState("");
  const [newUrl, setNewUrl] = useState("");
  const [newName, setNewName] = useState("");
  const [job, setJob] = useState<{ id: string; pct: number; phase: string } | null>(null);
  const [searching, setSearching] = useState(false);
  const [offline, setOffline] = useState<string | null>(null);

  useEffect(() => {
    api.storeSearch("").then(setFeatured).catch(() => setFeatured([]));
    api.listOpdsCatalogs().then(setCatalogs).catch(() => setCatalogs([]));
    return onEvent((e) => {
      if (e.name === "store:progress") {
        setJob({ id: String(e.payload.jobId), pct: Number(e.payload.pct), phase: String(e.payload.phase) });
      }
    });
  }, []);

  const search = useCallback(async () => {
    if (!query.trim()) return;
    setSearching(true);
    setOffline(null);
    try {
      const items = await api.storeSearch(query.trim());
      setResults(items.filter((i) => i.source === "gutenberg"));
      if (items.filter((i) => i.source === "gutenberg").length === 0) {
        setOffline("NO RESULTS — GUTENDEX MAY BE UNREACHABLE FROM THIS NETWORK");
      }
    } catch (e) {
      setOffline(String((e as Error).message).toUpperCase().slice(0, 70));
    } finally {
      setSearching(false);
    }
  }, [query]);

  const browseSe = useCallback(async (id: string) => {
    setSearching(true);
    setOffline(null);
    try {
      setSeItems(await api.storeBrowse(id));
    } catch (e) {
      setOffline(String((e as Error).message).toUpperCase().slice(0, 70));
    } finally {
      setSearching(false);
    }
  }, []);

  const download = useCallback(
    async (item: StoreItem) => {
      setJob({ id: item.id, pct: 0, phase: "download" });
      try {
        await api.storeDownload(item);
        await refreshBooks();
        showToast("ON YOUR SHELF ✓ — READY TO CHAT");
        nav("library");
      } catch (e) {
        showToast(String((e as Error).message).toUpperCase().slice(0, 60));
      } finally {
        setJob(null);
      }
    },
    [refreshBooks, showToast, nav],
  );

  const fetchAo3 = useCallback(async () => {
    if (!ao3.trim()) return;
    setJob({ id: "ao3", pct: 0, phase: "download" });
    try {
      await api.importAo3Link(ao3.trim());
      await refreshBooks();
      showToast("FORGED ✓ — ON YOUR SHELF");
      nav("library");
    } catch (e) {
      showToast(String((e as Error).message).toUpperCase().slice(0, 60));
    } finally {
      setJob(null);
    }
  }, [ao3, refreshBooks, showToast, nav]);

  return (
    <div className="p-4 lg:p-8 max-w-5xl mx-auto">
      <h1 className="v-headline text-4xl lg:text-5xl mb-1">THE STORE</h1>
      <MetaRow>
        <span>EVERYTHING DOWNLOADS TO THIS DEVICE</span>
        <span>·</span>
        <span className="text-(--cyan) font-semibold">NOTHING PHONES HOME</span>
      </MetaRow>

      {job && (
        <div className="v-panel-shadow p-3 mt-4">
          <div className="v-meta mb-1">{job.phase.toUpperCase()} · {job.id.toUpperCase()}</div>
          <Progress pct={job.pct} animate />
        </div>
      )}

      {/* FEATURED — vena-catalog */}
      <section className="mt-6">
        <div className="f-cond text-lg">FEATURED</div>
        <div className="v-meta mb-3">CURATED PACKAGES · LEDGER PRE-FORGED · ONE TAP TO CHAT</div>
        <div className="grid sm:grid-cols-2 gap-3">
          {featured.filter((f) => f.source === "vena-catalog").map((f) => (
            <div key={f.id} className="v-panel-shadow p-4 flex items-center justify-between gap-3">
              <div>
                <div className="f-cond">{f.title}</div>
                <MetaRow>
                  <span>{(f.author ?? "").toUpperCase()}</span>
                  <span>·</span>
                  <span>{(f.license ?? "").toUpperCase()}</span>
                </MetaRow>
              </div>
              {f.on_shelf ? (
                <span className="v-meta text-(--cyan) font-semibold">ON YOUR SHELF ✓</span>
              ) : (
                <button className="v-btn v-btn-red text-xs" onClick={() => download(f)}>GET</button>
              )}
            </div>
          ))}
          {featured.length === 0 && <div className="v-meta">FEATURED SHELF UNAVAILABLE</div>}
        </div>
      </section>

      {/* PROJECT GUTENBERG */}
      <section className="mt-8">
        <div className="f-cond text-lg">PROJECT GUTENBERG</div>
        <div className="v-meta mb-3">70,000+ TITLES · SEARCH FIRST, BROWSE SECOND</div>
        <div className="flex gap-2">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && search()}
            placeholder="Search the public domain…"
            className="flex-1 v-keyline bg-(--bub) px-3 py-2 f-serif outline-none"
          />
          <button className="v-btn" disabled={searching} onClick={search}>
            {searching ? "SEARCHING…" : "SEARCH"}
          </button>
        </div>
        {offline && <div className="v-meta text-(--red) mt-2">{offline}</div>}
        <div className="grid sm:grid-cols-2 gap-3 mt-3">
          {results.map((r) => (
            <div key={r.id} className="v-panel p-3 flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="f-cond text-sm truncate">{r.title}</div>
                <div className="v-meta truncate">{(r.author ?? "").toUpperCase()} · PUBLIC DOMAIN</div>
              </div>
              <button className="v-btn text-xs" disabled={!r.download_url} onClick={() => download(r)}>GET</button>
            </div>
          ))}
        </div>
      </section>

      {/* STANDARD EBOOKS + YOUR CATALOGS */}
      <section className="mt-8">
        <div className="f-cond text-lg">STANDARD EBOOKS</div>
        <div className="v-meta mb-2">BEAUTIFULLY FORMATTED · HAND-PROOFREAD EDITIONS</div>
        <button className="v-btn text-xs" onClick={() => browseSe("standard-ebooks")}>BROWSE THE CATALOG →</button>
        <div className="grid sm:grid-cols-2 gap-3 mt-3">
          {seItems.map((r) => (
            <div key={r.id} className="v-panel p-3 flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="f-cond text-sm truncate">{r.title}</div>
                <div className="v-meta truncate">{(r.author ?? "").toUpperCase()} · STANDARD EBOOKS EDITION</div>
              </div>
              <button className="v-btn text-xs" disabled={!r.download_url} onClick={() => download(r)}>GET</button>
            </div>
          ))}
        </div>

        <div className="f-cond text-lg mt-8">YOUR CATALOGS</div>
        <div className="v-meta mb-2">OPDS · SELF-HOSTED CALIBRE, FEEDBOOKS, YOUR OWN SERVER</div>
        {catalogs.map((c) => (
          <div key={c.id} className="v-panel p-2 flex items-center justify-between gap-2 mb-2">
            <div className="v-meta truncate">{c.name.toUpperCase()} · {c.url}</div>
            <div className="flex gap-1">
              <button className="v-btn text-xs" onClick={() => browseSe(c.id)}>BROWSE</button>
              {c.id !== "standard-ebooks" && (
                <button
                  className="v-btn text-xs"
                  onClick={async () => {
                    await api.removeOpdsCatalog(c.id);
                    setCatalogs(await api.listOpdsCatalogs());
                  }}
                >
                  REMOVE
                </button>
              )}
            </div>
          </div>
        ))}
        <div className="flex gap-2 mt-2 flex-wrap">
          <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="Name"
            className="v-keyline bg-(--bub) px-2 py-1 f-serif text-sm outline-none w-32" />
          <input value={newUrl} onChange={(e) => setNewUrl(e.target.value)} placeholder="https://…/opds"
            className="flex-1 v-keyline bg-(--bub) px-2 py-1 f-serif text-sm outline-none min-w-40" />
          <button
            className="v-btn text-xs"
            onClick={async () => {
              if (!newUrl.trim()) return;
              await api.addOpdsCatalog(newUrl.trim(), newName.trim() || "My catalog");
              setCatalogs(await api.listOpdsCatalogs());
              setNewUrl(""); setNewName("");
              showToast("CATALOG ADDED");
            }}
          >
            ADD A CATALOG
          </button>
        </div>
      </section>

      {/* AO3 */}
      <section className="mt-8 mb-10">
        <div className="f-cond text-lg">READING FANFICTION?</div>
        <div className="v-meta mb-2">PASTE AN AO3 WORK LINK — VENA FETCHES THE EPUB AO3 ITSELF SERVES</div>
        <div className="flex gap-2">
          <input
            value={ao3}
            onChange={(e) => setAo3(e.target.value)}
            placeholder="https://archiveofourown.org/works/…"
            className="flex-1 v-keyline bg-(--bub) px-3 py-2 f-serif outline-none"
          />
          <button className="v-btn v-btn-cyan" onClick={fetchAo3}>FETCH</button>
        </div>
      </section>
    </div>
  );
}
