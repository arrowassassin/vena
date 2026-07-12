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
use std::path::Path;
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
            .build()
            .expect("http client")
    })
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
    let part = dest.with_extension("part");
    let already: u64 = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut reqb = client().get(url);
    if already > 0 {
        reqb = reqb.header("Range", format!("bytes={already}-"));
    }
    let mut resp = reqb
        .send()
        .map_err(|e| VenaError::Other(format!("download failed: {e}")))?;
    let status = resp.status();
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !status.is_success() {
        return Err(VenaError::Other(format!("download HTTP {status}")));
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
        if total > 0 {
            on_progress(((downloaded * 100) / total).min(99) as u32);
        }
    }
    drop(file);

    // Integrity gate: verify BEFORE renaming into place / marking ready.
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

/// Download a tier's GGUF from Hugging Face with REAL SHA-256 verification: the
/// expected digest comes from the model's Git-LFS pointer (`oid sha256:<hex>`,
/// served at /raw/), the blob downloads resumably, and it is verified before being
/// renamed into place / marked ready (§11.4 plumbing).
pub fn download_hf_gguf(model: &str, dir: &Path, on_progress: &mut dyn FnMut(u32)) -> Result<()> {
    let (repo, file) = hf_repo_file(model)
        .ok_or_else(|| VenaError::Other(format!("no HF mapping for {model}")))?;
    let pointer_url = format!("https://huggingface.co/{repo}/raw/main/{file}");
    let pointer = client()
        .get(&pointer_url)
        .send()
        .and_then(|r| r.text())
        .map_err(|e| VenaError::Other(format!("fetching LFS pointer: {e}")))?;
    let expected = pointer
        .lines()
        .find_map(|l| l.strip_prefix("oid sha256:"))
        .map(str::trim)
        .map(str::to_string);

    let url = format!("https://huggingface.co/{repo}/resolve/main/{file}?download=true");
    let dest = dir.join(format!("{model}.gguf"));
    download_file_verified(&url, &dest, expected.as_deref(), &[], on_progress)
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
pub fn gutendex_search(
    query: &str,
    page: u32,
) -> Result<
    Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )>,
> {
    let url = format!(
        "https://gutendex.com/books?search={}&page={}",
        urlencode(query),
        page.max(1)
    );
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
pub fn opds_fetch(
    url: &str,
    user_hosts: &[String],
) -> Result<Vec<(String, String, Option<String>, Option<String>)>> {
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

pub fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Enforce the §11.2 network allowlist. Fixed sources + explicit user-configured
/// hosts only; anything else is refused with `NetworkNotAllowed`.
fn assert_allowed(url: &str, user_hosts: &[String]) -> Result<()> {
    let host = host_of(url);
    const ALLOWED_SUFFIXES: &[&str] = &[
        "gutendex.com",
        "gutenberg.org",
        "standardebooks.org",
        "archiveofourown.org",
        "huggingface.co",
        "hf.co",
    ];
    let fixed = ALLOWED_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")));
    let user = user_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host));
    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]");
    if fixed || user || local {
        Ok(())
    } else {
        Err(VenaError::NetworkNotAllowed(host))
    }
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
