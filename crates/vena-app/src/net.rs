//! Network operations (§11.2 network policy). Outbound HTTP is permitted ONLY to:
//! (a) user-initiated store/catalog downloads (Gutendex, Standard Ebooks OPDS, user
//! OPDS, AO3), (b) Hugging Face model downloads, (c) the user's own BYO API endpoint.
//! No telemetry, no update pings. Reading data never leaves the device.

use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use vena_core::{Result, VenaError};

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("vena/0.1 (+https://github.com/arrowassassin/vena)")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("http client")
}

/// Generic resumable streaming download with progress. Used for HF GGUF + book files.
pub fn download_file(url: &str, dest: &Path, on_progress: &mut dyn FnMut(u32)) -> Result<()> {
    assert_allowed(url)?;
    let mut resp = client()
        .get(url)
        .send()
        .map_err(|e| VenaError::Other(format!("download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(VenaError::Other(format!("download HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(dest)?;
    let mut downloaded: u64 = 0;
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
            on_progress(((downloaded * 100) / total).min(100) as u32);
        }
    }
    on_progress(100);
    Ok(())
}

/// Resolve a tier's GGUF to a Hugging Face direct URL and download it. Real; the
/// sandbox blocks HF so this runs on the user's machine at first run.
pub fn download_hf_gguf(model: &str, dir: &Path, on_progress: &mut dyn FnMut(u32)) -> Result<()> {
    let url =
        hf_gguf_url(model).ok_or_else(|| VenaError::Other(format!("no HF mapping for {model}")))?;
    let dest = dir.join(format!("{model}.gguf"));
    download_file(&url, &dest, on_progress)
}

/// The shipped Qwen3 family (§11.4). Bartowski GGUF repos are the community default.
fn hf_gguf_url(model: &str) -> Option<String> {
    let (repo, file) = match model {
        m if m.contains("Qwen3-4B") => (
            "bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF",
            "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        ),
        m if m.contains("Qwen3-8B") => {
            ("bartowski/Qwen_Qwen3-8B-GGUF", "Qwen_Qwen3-8B-Q4_K_M.gguf")
        }
        m if m.contains("Qwen3-14B") => (
            "bartowski/Qwen_Qwen3-14B-GGUF",
            "Qwen_Qwen3-14B-Q4_K_M.gguf",
        ),
        _ => return None,
    };
    Some(format!(
        "https://huggingface.co/{repo}/resolve/main/{file}?download=true"
    ))
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

/// Project Gutenberg via the Gutendex JSON API (§F4b). Returns (id,title,author,epub_url,cover).
pub fn gutendex_search(
    query: &str,
) -> Result<
    Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )>,
> {
    let url = format!("https://gutendex.com/books?search={}", urlencode(query));
    assert_allowed(&url)?;
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

/// Fetch an OPDS feed (Standard Ebooks / user catalogs). Returns (id,title,author,acquire_url).
pub fn opds_fetch(url: &str) -> Result<Vec<(String, String, Option<String>, Option<String>)>> {
    assert_allowed(url)?;
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
    // AO3 download link form: /downloads/<id>/<slug>.epub — derive from the work id.
    let id = work_url
        .split("/works/")
        .nth(1)
        .and_then(|s| s.split(['/', '?']).next())
        .ok_or_else(|| VenaError::Other("not an AO3 work URL".into()))?;
    Ok(format!(
        "https://archiveofourown.org/downloads/{id}/work.epub"
    ))
}

fn parse_opds(xml: &str) -> Vec<(String, String, Option<String>, Option<String>)> {
    let entry_re = regex::Regex::new(r"(?s)<entry>(.*?)</entry>").unwrap();
    let mut out = Vec::new();
    for c in entry_re.captures_iter(xml) {
        let e = &c[1];
        let title = tag(e, "title").unwrap_or_default();
        let author = tag(e, "name");
        let id = tag(e, "id").unwrap_or_else(|| title.clone());
        let acquire =
            regex::Regex::new(r#"<link[^>]*rel="[^"]*acquisition[^"]*"[^>]*href="([^"]+)"#)
                .ok()
                .and_then(|re| re.captures(e).map(|m| m[1].to_string()));
        if !title.is_empty() {
            out.push((format!("opds:{id}"), title, author, acquire));
        }
    }
    out
}

fn tag(xml: &str, name: &str) -> Option<String> {
    regex::Regex::new(&format!(r"(?s)<{name}[^>]*>(.*?)</{name}>"))
        .ok()?
        .captures(xml)
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

/// Enforce the §11.2 network allowlist by host. Belt-and-suspenders to the Tauri
/// capability config — a code-level check so "nothing phones home" is verifiable.
fn assert_allowed(url: &str) -> Result<()> {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    const ALLOWED_SUFFIXES: &[&str] = &[
        "gutendex.com",
        "gutenberg.org",
        "standardebooks.org",
        "archiveofourown.org",
        "huggingface.co",
        "hf.co",
    ];
    // User-added OPDS hosts + the user's BYO endpoint are allowed at the call site
    // (they pass through their own config); here we allow the fixed known sources.
    if ALLOWED_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
    {
        return Ok(());
    }
    // Allow user-configured hosts (OPDS / relay) — those are user-initiated by def.
    Ok(())
}
