//! Network operations (§11.2 network policy). Outbound HTTP is permitted ONLY to:
//! (a) user-initiated store/catalog downloads (Gutendex, Standard Ebooks OPDS, user
//! OPDS, AO3), (b) Hugging Face model downloads, (c) the user's own BYO API endpoint.
//! No telemetry, no update pings. Reading data never leaves the device.
//!
//! `assert_allowed` ENFORCES the allowlist: fixed known sources are always allowed;
//! any other host must be explicitly passed by the caller as a user-configured host
//! (their OPDS catalogs / BYO endpoint). Unknown hosts are rejected.

use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type PathSet = Mutex<std::collections::HashSet<PathBuf>>;
/// (gutenberg_id, title, author, epub_url, cover_url) — a Gutendex search hit.
type GutendexHit = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
/// (entry_id, title, author, acquisition_url) — an OPDS catalog entry.
type OpdsEntry = (String, String, Option<String>, Option<String>);
use vena_core::{Result, VenaError};

/// One shared client for the whole process — reuses the connection pool / keep-alive
/// across every store search, catalog fetch, and model download instead of paying a
/// fresh TLS handshake per request.
fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent("vena/0.1 (+https://github.com/arrowassassin/vena)")
            .timeout(std::time::Duration::from_secs(60))
            .redirect(guarded_redirect())
            .build()
            .expect("http client")
    })
}

/// Client for multi-GB model downloads. reqwest's `timeout` is a TOTAL request
/// deadline — with the 60s general client a 4 GB file died mid-stream on any
/// normal connection. Downloads get a connect guard and NO overall cap; a dead
/// peer is caught by TCP keepalive, and an interrupted download resumes from
/// its .part on the next attempt.
fn dl_client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent("vena/0.1 (+https://github.com/arrowassassin/vena)")
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(None)
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .redirect(guarded_redirect())
            .build()
            .expect("dl client")
    })
}

/// Quick liveness probe for a local OpenAI-compatible server (Ollama, LM
/// Studio, llama-server). 2s budget — used as a pre-flight so a dead socket
/// becomes an honest "engine offline" message instead of a mid-turn failure.
pub fn probe_openai_base(base: &str) -> bool {
    let root = base
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/');
    let url = format!("{root}/v1/models");
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()
        .and_then(|c| c.get(&url).send().ok())
        // 404 still proves a server is listening (some lack /v1/models)
        .map(|r| r.status().is_success() || r.status().as_u16() == 404)
        .unwrap_or(false)
}

/// Cancellation registry for in-flight downloads, keyed by destination path.
/// `cancel_download(dest)` flags it; the streaming loop notices between chunks
/// and stops, KEEPING the .part file so the next attempt resumes.
fn cancels() -> &'static PathSet {
    static C: std::sync::OnceLock<PathSet> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub fn cancel_download(dest: &Path) {
    cancels().lock().unwrap().insert(dest.to_path_buf());
}

fn take_cancel(dest: &Path) -> bool {
    cancels().lock().unwrap().remove(dest)
}

/// In-flight registry: a destination can only be downloaded by ONE worker at a
/// time. A page refresh forgets the UI's downloading state — clicking RESUME
/// while the first worker is still streaming/verifying must not race it.
fn inflight() -> &'static PathSet {
    static C: std::sync::OnceLock<PathSet> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

struct InflightGuard(std::path::PathBuf);
impl Drop for InflightGuard {
    fn drop(&mut self) {
        inflight().lock().unwrap().remove(&self.0);
    }
}

/// RESUMABLE streaming download. Partial data lands in `<dest>.part`; re-invocation
/// continues with a Range request; the file is renamed into place only when complete
/// (and, when a digest is supplied, SHA-256-verified).
pub fn download_file(url: &str, dest: &Path, on_progress: &mut dyn FnMut(u32)) -> Result<()> {
    download_file_verified(url, dest, None, &[], on_progress)
}

pub fn download_file_verified(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    user_hosts: &[String],
    on_progress: &mut dyn FnMut(u32),
) -> Result<()> {
    assert_allowed(url, user_hosts)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // One worker per destination — a raced second attempt (page refresh +
    // RESUME while the first is still verifying) errors honestly instead of
    // corrupting the .part or 416-ing against its own complete bytes.
    let _guard = {
        let mut inf = inflight().lock().unwrap();
        if !inf.insert(dest.to_path_buf()) {
            return Err(VenaError::Other(
                "this download is already running — give it a moment".into(),
            ));
        }
        InflightGuard(dest.to_path_buf())
    };
    let _ = take_cancel(dest); // clear any stale flag from a finished run
    let part = dest.with_extension("part");
    let already: u64 = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut bytes_complete = false;
    let mut reqb = dl_client().get(url);
    if already > 0 {
        reqb = reqb.header("Range", format!("bytes={already}-"));
    }
    let mut resp = reqb
        .send()
        .map_err(|e| VenaError::Other(format!("download failed: {e}")))?;
    let status = resp.status();
    // 416 Range Not Satisfiable with a non-empty .part means our bytes already
    // reach or exceed the server's length. With an expected SHA we can trust
    // the integrity gate below to accept or reject them. WITHOUT a SHA (EPUB
    // downloads) a 416 could equally mean the .part is corrupt/oversized, so we
    // must NOT rename it blind — discard and restart fresh.
    if status.as_u16() == 416 && already > 0 {
        if expected_sha256.is_some() {
            bytes_complete = true;
        } else {
            let _ = std::fs::remove_file(&part);
            return Err(VenaError::Other(
                "the partial download didn't match the source — restarting; tap download again"
                    .into(),
            ));
        }
    }
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !bytes_complete {
        if !status.is_success() {
            let hint = if status.as_u16() == 401 || status.as_u16() == 404 {
                " — the file isn't at the expected Hugging Face path (the repo may have moved)"
            } else {
                ""
            };
            return Err(VenaError::Other(format!("download HTTP {status}{hint}")));
        }
        let remaining = resp.content_length().unwrap_or(0);
        let total = if resuming {
            already + remaining
        } else {
            remaining
        };

        let mut file = if resuming {
            std::fs::OpenOptions::new().append(true).open(&part)?
        } else {
            std::fs::File::create(&part)?
        };
        let mut downloaded: u64 = if resuming { already } else { 0 };
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = std::io::Read::read(&mut resp, &mut buf)
                .map_err(|e| VenaError::Other(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n as u64;
            if take_cancel(dest) {
                return Err(VenaError::Other(
                    "download stopped — the partial file is kept; RESUME picks up from here".into(),
                ));
            }
            if total > 0 {
                // bytes stop at 98 — 99 is the SHA-verify phase (hashing a
                // multi-GB file takes real seconds; the UI shows VERIFYING)
                on_progress(
                    (downloaded
                        .saturating_mul(100)
                        .checked_div(total)
                        .unwrap_or(0))
                    .min(98) as u32,
                );
            }
        }
        drop(file);
    }

    // Integrity gate: verify BEFORE renaming into place / marking ready.
    on_progress(99);
    if let Some(expected) = expected_sha256 {
        let actual = vena_core::hash::sha256_hex_reader(std::fs::File::open(&part)?)?;
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&part);
            return Err(VenaError::Other(format!(
                "SHA-256 mismatch (expected {expected}, got {actual}) — download discarded"
            )));
        }
    }
    std::fs::rename(&part, dest)?;
    on_progress(100);
    Ok(())
}

/// Resolve the actual .gguf filename inside an HF repo. Prefers the exact
/// `prefer` name when the repo lists it; otherwise falls back through common
/// quantizations. Repo layouts drift — a hardcoded filename 401s forever,
/// while the live listing self-corrects. On API failure returns `prefer`
/// unchanged so a correct hardcoded name still downloads offline-of-the-API.
pub fn hf_pick_gguf(repo: &str, prefer: &str) -> String {
    let url = format!("https://huggingface.co/api/models/{repo}");
    let names: Vec<String> = client()
        .get(&url)
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .and_then(|v| {
            v["siblings"].as_array().map(|a| {
                a.iter()
                    .filter_map(|s| s["rfilename"].as_str())
                    .filter(|n| n.ends_with(".gguf") && !n.to_lowercase().contains("vae"))
                    .map(str::to_string)
                    .collect()
            })
        })
        .unwrap_or_default();
    if names.is_empty() || names.iter().any(|n| n == prefer) {
        return prefer.to_string();
    }
    for pat in ["Q8_0", "q8_0", "Q5", "Q4", "f16", "F16"] {
        if let Some(n) = names.iter().find(|n| n.contains(pat)) {
            return n.clone();
        }
    }
    names
        .into_iter()
        .next()
        .unwrap_or_else(|| prefer.to_string())
}

/// Download one HF file with REAL SHA-256 verification: the expected digest
/// comes from the file's Git-LFS pointer (`oid sha256:<hex>`, served at
/// /raw/), the blob downloads resumably, and it is verified before being
/// renamed into place (§11.4 plumbing).
pub fn hf_download(
    repo: &str,
    remote_file: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u32),
) -> Result<()> {
    let pointer_url = format!("https://huggingface.co/{repo}/raw/main/{remote_file}");
    let resp = client()
        .get(&pointer_url)
        .send()
        .map_err(|e| VenaError::Other(format!("fetching LFS pointer: {e}")))?;
    if !resp.status().is_success() {
        return Err(VenaError::Other(format!(
            "Hugging Face returned {} for {remote_file} — the model may have moved or be rate-limited; try again",
            resp.status()
        )));
    }
    let pointer = resp
        .text()
        .map_err(|e| VenaError::Other(format!("reading LFS pointer: {e}")))?;
    // GGUF weights are ALWAYS Git-LFS — a missing digest means we fetched an
    // error page / HTML, not the pointer. Refuse rather than install a
    // multi-GB blob unverified (a corrupt or wrong model marked "ready").
    let expected = pointer
        .lines()
        .find_map(|l| l.strip_prefix("oid sha256:"))
        .map(str::trim)
        .ok_or_else(|| {
            VenaError::Other(
                "couldn't read the model's integrity digest from Hugging Face — refusing to \
                 install an unverified download; try again in a moment"
                    .into(),
            )
        })?;
    let url = format!("https://huggingface.co/{repo}/resolve/main/{remote_file}?download=true");
    download_file_verified(&url, dest, Some(expected), &[], on_progress)
}

/// Download a tier's GGUF from Hugging Face (§11.4 plumbing). The in-repo
/// filename is resolved live so quantization renames don't strand the tier.
pub fn download_hf_gguf(model: &str, dir: &Path, on_progress: &mut dyn FnMut(u32)) -> Result<()> {
    let (repo, file) = hf_repo_file(model)
        .ok_or_else(|| VenaError::Other(format!("no HF mapping for {model}")))?;
    let remote = hf_pick_gguf(repo, file);
    let dest = dir.join(format!("{model}.gguf"));
    hf_download(repo, &remote, &dest, on_progress)
}

/// The shipped Qwen3 family (§11.4). Bartowski GGUF repos are the community default.
fn hf_repo_file(model: &str) -> Option<(&'static str, &'static str)> {
    match model {
        m if m.contains("Qwen3-4B") => Some((
            "bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF",
            "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        )),
        m if m.contains("Qwen3-8B") => {
            Some(("bartowski/Qwen_Qwen3-8B-GGUF", "Qwen_Qwen3-8B-Q4_K_M.gguf"))
        }
        m if m.contains("Qwen3-14B") => Some((
            "bartowski/Qwen_Qwen3-14B-GGUF",
            "Qwen_Qwen3-14B-Q4_K_M.gguf",
        )),
        _ => None,
    }
}

// ---------- Store sources (§F4) ----------

#[derive(Debug, Deserialize)]
struct GutendexPage {
    results: Vec<GutendexBook>,
}
#[derive(Debug, Deserialize)]
struct GutendexBook {
    id: i64,
    title: String,
    #[serde(default)]
    authors: Vec<GutendexAuthor>,
    #[serde(default)]
    formats: std::collections::HashMap<String, String>,
}
#[derive(Debug, Deserialize)]
struct GutendexAuthor {
    name: String,
}

/// Project Gutenberg via the Gutendex JSON API (§F4b). `page` = real pagination.
pub fn gutendex_search(query: &str, topic: Option<&str>, page: u32) -> Result<Vec<GutendexHit>> {
    let mut url = format!(
        "https://gutendex.com/books?search={}&page={}",
        urlencode(query),
        page.max(1)
    );
    if let Some(t) = topic {
        // Gutendex topic filter (subjects/bookshelves); its default sort is already
        // by popularity (download count), so an empty search = "most downloaded".
        url.push_str(&format!("&topic={}", urlencode(t)));
    }
    assert_allowed(&url, &[])?;
    let resp = client()
        .get(&url)
        .send()
        .map_err(|e| VenaError::Other(e.to_string()))?;
    let page: GutendexPage = resp.json().map_err(|e| VenaError::Other(e.to_string()))?;
    Ok(page
        .results
        .into_iter()
        .map(|b| {
            let epub = b
                .formats
                .iter()
                .find(|(k, _)| k.contains("epub"))
                .map(|(_, v)| v.clone());
            let cover = b
                .formats
                .iter()
                .find(|(k, _)| k.contains("image"))
                .map(|(_, v)| v.clone());
            (
                format!("gutenberg:{}", b.id),
                b.title,
                b.authors.first().map(|a| a.name.clone()),
                epub,
                cover,
            )
        })
        .collect())
}

/// Fetch an OPDS feed. `user_hosts` = hosts of the user's registered catalogs; a
/// feed on any other non-fixed host is refused (policy enforcement).
pub fn opds_fetch(url: &str, user_hosts: &[String]) -> Result<Vec<OpdsEntry>> {
    assert_allowed(url, user_hosts)?;
    let body = client()
        .get(url)
        .header("accept", "application/atom+xml")
        .send()
        .map_err(|e| VenaError::Other(e.to_string()))?
        .text()
        .map_err(|e| VenaError::Other(e.to_string()))?;
    Ok(parse_opds(&body))
}

/// AO3 serves an EPUB per work officially — fetch that URL (user-initiated, §F4).
pub fn ao3_epub_url(work_url: &str) -> Result<String> {
    let id = work_url
        .split("/works/")
        .nth(1)
        .and_then(|s| s.split(['/', '?']).next())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .ok_or_else(|| VenaError::Other("not an AO3 work URL".into()))?;
    Ok(format!(
        "https://archiveofourown.org/downloads/{id}/work.epub"
    ))
}

fn parse_opds(xml: &str) -> Vec<(String, String, Option<String>, Option<String>)> {
    // Regexes compiled ONCE (a large OPDS feed has ~1000 entries; compiling per
    // entry was orders of magnitude costlier than the matches themselves).
    use std::sync::OnceLock;
    static ENTRY_RE: OnceLock<regex::Regex> = OnceLock::new();
    static ACQUIRE_RE: OnceLock<regex::Regex> = OnceLock::new();
    let entry_re = ENTRY_RE.get_or_init(|| regex::Regex::new(r"(?s)<entry>(.*?)</entry>").unwrap());
    let acquire_re = ACQUIRE_RE.get_or_init(|| {
        regex::Regex::new(r#"<link[^>]*rel="[^"]*acquisition[^"]*"[^>]*href="([^"]+)"#).unwrap()
    });

    let mut out = Vec::new();
    for c in entry_re.captures_iter(xml) {
        let e = &c[1];
        let title = tag(e, "title").unwrap_or_default();
        let author = tag(e, "name");
        let id = tag(e, "id").unwrap_or_else(|| title.clone());
        let acquire = acquire_re.captures(e).map(|m| m[1].to_string());
        if !title.is_empty() {
            out.push((format!("opds:{id}"), title, author, acquire));
        }
    }
    out
}

fn tag(xml: &str, name: &str) -> Option<String> {
    // Cache one compiled regex per tag name across calls (title/name/id repeat for
    // every OPDS entry).
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let re = {
        let mut map = cache.lock().unwrap();
        map.entry(name.to_string())
            .or_insert_with(|| {
                regex::Regex::new(&format!(r"(?s)<{name}[^>]*>(.*?)</{name}>")).unwrap()
            })
            .clone()
    };
    re.captures(xml)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Robust host extraction — one parser, shared with the remote/loopback
/// classifier, immune to `userinfo@` authority spoofing.
pub fn host_of(url: &str) -> String {
    vena_core::util::url_host(url)
}

/// Fixed §11.2 sources: HF (+ its LFS CDN under hf.co) and the public-domain
/// book sources. Redirect hops are validated against this same list.
const ALLOWED_SUFFIXES: &[&str] = &[
    "gutendex.com",
    "gutenberg.org",
    "standardebooks.org",
    "archiveofourown.org",
    "huggingface.co",
    "hf.co",
];

fn host_in_fixed(host: &str) -> bool {
    ALLOWED_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

/// Enforce the §11.2 network allowlist. Fixed sources + explicit user-configured
/// hosts only; anything else is refused with `NetworkNotAllowed`.
fn assert_allowed(url: &str, user_hosts: &[String]) -> Result<()> {
    let host = host_of(url);
    let user = user_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host));
    if host_in_fixed(&host) || user || vena_core::util::is_loopback_host(&host) {
        Ok(())
    } else {
        Err(VenaError::NetworkNotAllowed(host))
    }
}

/// Redirect policy for the shared clients: a redirect may only land on a fixed
/// allowlisted host (or loopback). A 302 from an allowed origin to an arbitrary
/// host would otherwise escape assert_allowed, which only saw the initial URL.
/// User-OPDS hosts are not carried here, so a cross-host OPDS redirect fails
/// closed — the safe default.
fn guarded_redirect() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("too many redirects");
        }
        let host = vena_core::util::url_host(attempt.url().as_str());
        if host_in_fixed(&host) || vena_core::util::is_loopback_host(&host) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn allowlist_rejects_unknown_hosts() {
        assert!(super::assert_allowed("https://gutendex.com/books?search=x", &[]).is_ok());
        assert!(super::assert_allowed("https://www.gutenberg.org/e/1.epub", &[]).is_ok());
        assert!(super::assert_allowed("https://evil.example.com/x", &[]).is_err());
        // user-registered OPDS host is allowed
        assert!(
            super::assert_allowed("https://my.calibre.net/opds", &["my.calibre.net".into()])
                .is_ok()
        );
        // suffix spoofing is rejected
        assert!(super::assert_allowed("https://notgutenberg.org.evil.com/x", &[]).is_err());
    }

    #[test]
    fn ao3_url_requires_numeric_work_id() {
        assert!(super::ao3_epub_url("https://archiveofourown.org/works/12345").is_ok());
        assert!(super::ao3_epub_url("https://archiveofourown.org/users/foo").is_err());
    }
}
