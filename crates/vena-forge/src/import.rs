//! Import: EPUB (first-class) + plain text/Markdown (Gutenberg-aware) → chapters.
//! Also deterministic format detection (§F5c: prose | comic | illustrated-prose).
//! Canon text is preserved verbatim as HTML — never AI-generated or modified.

use anyhow::{anyhow, Result};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Chapter {
    pub seq: i64,
    pub title: Option<String>,
    pub paragraphs: Vec<String>,
}

impl Chapter {
    pub fn word_count(&self) -> usize {
        self.paragraphs
            .iter()
            .map(|p| p.split_whitespace().count())
            .sum()
    }
    /// Canon HTML — pristine paragraphs, no AI affordances inline.
    pub fn content_html(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| format!("<p>{}</p>", html_escape(p)))
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn est_minutes(&self) -> i64 {
        ((self.word_count() as f64 / 220.0).ceil() as i64).max(1)
    }
}

#[derive(Debug, Clone)]
pub struct ImportedBook {
    pub title: String,
    pub author: Option<String>,
    pub chapters: Vec<Chapter>,
    pub cover: Option<Vec<u8>>,
    pub cover_name: Option<String>,
    /// prose | comic | illustrated-prose
    pub profile: String,
    /// human-readable detection evidence ("~1100 words/chapter · 0 images")
    pub profile_evidence: String,
}

pub fn import_path(path: &Path) -> Result<ImportedBook> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("epub") => import_epub(path),
        Some("cbz") => import_cbz(path),
        Some("txt") | Some("text") | Some("md") | Some("markdown") | None => {
            let raw = std::fs::read_to_string(path)?;
            let (title, author) = (
                guess_title(&raw).unwrap_or_else(|| stem(path)),
                guess_author(&raw),
            );
            let chapters = split_text_into_chapters(&clean_gutenberg(&raw));
            if chapters.is_empty() {
                return Err(anyhow!("no chapters detected in {}", path.display()));
            }
            let words: usize = chapters.iter().map(|c| c.word_count()).sum();
            let evidence = format!(
                "{} chapters · ~{} words/chapter · 0 images",
                chapters.len(),
                words / chapters.len().max(1)
            );
            Ok(ImportedBook {
                title,
                author,
                chapters,
                cover: None,
                cover_name: None,
                profile: "prose".into(),
                profile_evidence: evidence,
            })
        }
        Some(other) => Err(anyhow!("unsupported format: .{other}")),
    }
}

// ---------- plain text (Gutenberg) ----------

/// Strip Project Gutenberg header/footer so only canon text remains.
pub fn clean_gutenberg(raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let raw = normalized.as_str();
    let mut body = raw;
    if let Some(idx) = raw.find("*** START OF") {
        if let Some(nl) = raw[idx..].find('\n') {
            body = &raw[idx + nl + 1..];
        }
    }
    if let Some(idx) = body.find("*** END OF") {
        body = &body[..idx];
    }
    body.to_string()
}

/// Split into chapters on CHAPTER/roman/numeric headers. Segments below a word
/// threshold are dropped — this discards a table-of-contents whose entries look
/// like chapter headers but carry no prose.
pub fn split_text_into_chapters(text: &str) -> Vec<Chapter> {
    // Leading whitespace is [ \t]* (NOT \s*) so it can't swallow the preceding
    // blank line and shift the cut onto an empty line.
    let header = regex::Regex::new(
        r"(?m)^[ \t]*(?:CHAPTER|Chapter|BOOK|PART)\s+(?:[IVXLCDM]+|\d+|[A-Z][a-z]+)\.?[ \t]*$",
    )
    .unwrap();

    let mut cuts: Vec<usize> = header.find_iter(text).map(|m| m.start()).collect();
    if cuts.is_empty() {
        // No headers → treat blank-line-separated blocks as one chapter.
        let paras = paragraphs(text);
        if paras.is_empty() {
            return vec![];
        }
        return vec![Chapter {
            seq: 1,
            title: None,
            paragraphs: paras,
        }];
    }
    cuts.push(text.len());

    let mut chapters = Vec::new();
    let mut seq = 1;
    for w in cuts.windows(2) {
        let seg = &text[w[0]..w[1]];
        let mut lines = seg.lines();
        let header_line = lines.next().unwrap_or("").trim().to_string(); // "CHAPTER I"
        let rest_start: String = lines.clone().collect::<Vec<_>>().join("\n");

        // A chapter's title is its optional ALL-CAPS subtitle (e.g. "JONATHAN
        // HARKER'S JOURNAL"), never another "CHAPTER n" header echo. Fall back to a
        // clean "Chapter <token>" from the header itself.
        let mut subtitle = None;
        let mut body_text = rest_start.clone();
        for line in rest_start.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if is_titleish(t) && !is_header_line(t) {
                subtitle = Some(titlecase(t));
                if let Some(pos) = body_text.find(line) {
                    body_text.replace_range(pos..pos + line.len(), "");
                }
            }
            break;
        }
        let paras = paragraphs(&body_text);
        let wc: usize = paras.iter().map(|p| p.split_whitespace().count()).sum();
        if wc < 150 {
            continue; // TOC stub or front-matter divider
        }
        let title = subtitle.or_else(|| clean_header_title(&header_line));
        chapters.push(Chapter {
            seq,
            title,
            paragraphs: paras,
        });
        seq += 1;
    }
    chapters
}

/// Split a text block into paragraphs on blank lines; join hard-wrapped lines.
fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|block| {
            block
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect()
}

fn is_header_line(line: &str) -> bool {
    let l = line.trim().to_uppercase();
    ["CHAPTER", "BOOK", "PART"].iter().any(|p| l.starts_with(p))
}

/// Normalize a header like "CHAPTER I" / "Chapter 3." into "Chapter I" / "Chapter 3".
fn clean_header_title(header: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)^\s*(CHAPTER|BOOK|PART)\s+([IVXLCDM]+|\d+|[A-Za-z]+)").ok()?;
    let c = re.captures(header)?;
    let word = titlecase(c.get(1)?.as_str());
    let token = c.get(2)?.as_str();
    // Keep roman numerals upper-case; title-case word numbers.
    let token = if token.chars().all(|ch| "IVXLCDMivxlcdm".contains(ch)) {
        token.to_uppercase()
    } else if token.chars().all(|ch| ch.is_ascii_digit()) {
        token.to_string()
    } else {
        titlecase(token)
    };
    Some(format!("{word} {token}"))
}

fn is_titleish(line: &str) -> bool {
    let letters: String = line.chars().filter(|c| c.is_alphabetic()).collect();
    !letters.is_empty()
        && letters.chars().all(|c| c.is_uppercase())
        && line.split_whitespace().count() <= 8
        && line.len() <= 60
}

// ---------- EPUB ----------

pub fn import_epub(path: &Path) -> Result<ImportedBook> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    // 1. META-INF/container.xml -> OPF path
    let opf_path = {
        let mut container = String::new();
        zip.by_name("META-INF/container.xml")?
            .read_to_string(&mut container)?;
        find_attr(&container, "rootfile", "full-path")
            .ok_or_else(|| anyhow!("EPUB: no rootfile in container.xml"))?
    };

    let opf = read_zip_string(&mut zip, &opf_path)?;
    let base = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

    let title = find_tag_text(&opf, "dc:title").unwrap_or_else(|| stem(path));
    let author = find_tag_text(&opf, "dc:creator");

    // 2. manifest id -> href; count images for format detection
    let manifest = parse_manifest(&opf);
    let image_count = manifest
        .values()
        .filter(|(_, mt)| mt.starts_with("image/"))
        .count();
    let pre_paginated = opf.contains("pre-paginated");
    let rtl = opf.contains("page-progression-direction=\"rtl\"");

    // cover
    let mut cover = None;
    let mut cover_name = None;
    if let Some((href, _)) = manifest.get("cover-image").or_else(|| {
        manifest
            .iter()
            .find(|(k, _)| k.contains("cover"))
            .map(|(_, v)| v)
            .and(None)
    }) {
        let full = join(base, href);
        if let Ok(bytes) = read_zip_bytes(&mut zip, &full) {
            cover_name = Some(href.rsplit('/').next().unwrap_or("cover").to_string());
            cover = Some(bytes);
        }
    }

    // 3. spine order -> chapters
    let spine = parse_spine(&opf);
    let mut chapters = Vec::new();
    let mut seq = 1;
    let mut total_chars = 0usize;
    for idref in &spine {
        let Some((href, mt)) = manifest.get(idref) else {
            continue;
        };
        if !mt.contains("html") && !mt.contains("xml") {
            continue;
        }
        let full = join(base, href);
        let Ok(xhtml) = read_zip_string(&mut zip, &full) else {
            continue;
        };
        let paras = html_to_paragraphs(&xhtml);
        let wc: usize = paras.iter().map(|p| p.split_whitespace().count()).sum();
        total_chars += paras.iter().map(|p| p.len()).sum::<usize>();
        if wc < 40 {
            continue; // nav / cover / copyright pages
        }
        let title = extract_html_title(&xhtml);
        chapters.push(Chapter {
            seq,
            title,
            paragraphs: paras,
        });
        seq += 1;
    }
    if chapters.is_empty() {
        return Err(anyhow!("EPUB: no readable spine documents"));
    }

    let spine_docs = chapters.len().max(1);
    let chars_per_doc = total_chars / spine_docs;
    let (profile, evidence) =
        detect_profile(pre_paginated, rtl, image_count, chars_per_doc, spine_docs);

    Ok(ImportedBook {
        title,
        author,
        chapters,
        cover,
        cover_name,
        profile,
        profile_evidence: evidence,
    })
}

/// §F5c deterministic format detection, in reliability order.
fn detect_profile(
    pre_paginated: bool,
    rtl: bool,
    image_count: usize,
    chars_per_doc: usize,
    spine_docs: usize,
) -> (String, String) {
    let rtl_note = if rtl { " · RTL" } else { "" };
    let profile = if pre_paginated {
        "comic"
    } else if chars_per_doc < 50 && image_count >= spine_docs {
        "comic"
    } else if image_count >= spine_docs && chars_per_doc > 50 {
        "illustrated-prose"
    } else {
        "prose"
    };
    let evidence = format!(
        "{}{} · {} images · ~{} chars/doc",
        if pre_paginated {
            "fixed-layout"
        } else {
            "reflowable"
        },
        rtl_note,
        image_count,
        chars_per_doc
    );
    (profile.to_string(), evidence)
}

// ---------- tiny XML/HTML helpers (canon is preserved, not parsed for meaning) ----------

fn read_zip_string(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let mut s = String::new();
    zip.by_name(name)?.read_to_string(&mut s)?;
    Ok(s)
}
fn read_zip_bytes(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut b = Vec::new();
    zip.by_name(name)?.read_to_end(&mut b)?;
    Ok(b)
}

fn join(base: &str, href: &str) -> String {
    if base.is_empty() {
        href.to_string()
    } else {
        format!("{base}/{href}")
    }
}

fn parse_manifest(opf: &str) -> std::collections::HashMap<String, (String, String)> {
    let re = regex::Regex::new(r#"<item\b[^>]*>"#).unwrap();
    let mut map = std::collections::HashMap::new();
    for m in re.find_iter(opf) {
        let tag = m.as_str();
        let id = attr(tag, "id");
        let href = attr(tag, "href");
        let mt = attr(tag, "media-type");
        let props = attr(tag, "properties");
        if let (Some(id), Some(href), Some(mt)) = (id, href, mt) {
            if props.as_deref() == Some("cover-image") {
                map.insert("cover-image".to_string(), (href.clone(), mt.clone()));
            }
            map.insert(id, (href, mt));
        }
    }
    map
}

fn parse_spine(opf: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"<itemref\b[^>]*>"#).unwrap();
    re.find_iter(opf)
        .filter_map(|m| attr(m.as_str(), "idref"))
        .collect()
}

/// Public: the app re-derives forge chapters from stored canon HTML (forge_ledger).
pub fn html_to_paragraphs(xhtml: &str) -> Vec<String> {
    // Extract <body>…</body>, split on block tags, strip remaining tags.
    let body = between(xhtml, "<body", "</body>").unwrap_or_else(|| xhtml.to_string());
    let block = regex::Regex::new(r"(?i)</(p|div|h[1-6]|li|br)\s*>|<br\s*/?>").unwrap();
    let chunked = block.replace_all(&body, "\n\n");
    let tag = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let text = tag.replace_all(&chunked, "");
    let decoded = html_unescape(&text);
    paragraphs(&decoded)
}

fn extract_html_title(xhtml: &str) -> Option<String> {
    for t in ["h1", "h2", "title"] {
        if let Some(inner) = between(xhtml, &format!("<{t}"), &format!("</{t}>")) {
            let tag = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
            let cleaned = tag.replace_all(&inner, "").trim().to_string();
            if !cleaned.is_empty() && cleaned.len() < 80 {
                return Some(html_unescape(&cleaned));
            }
        }
    }
    None
}

fn between(hay: &str, open: &str, close: &str) -> Option<String> {
    let start = hay.find(open)?;
    let after_open = hay[start..].find('>')? + start + 1;
    let end = hay[after_open..].find(close)? + after_open;
    Some(hay[after_open..end].to_string())
}

fn find_attr(xml: &str, tag: &str, attr_name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r#"<{tag}\b[^>]*>"#)).ok()?;
    let m = re.find(xml)?;
    attr(m.as_str(), attr_name)
}

fn find_tag_text(xml: &str, tag: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"(?s)<{tag}[^>]*>(.*?)</{tag}>")).ok()?;
    let c = re.captures(xml)?;
    let t = c.get(1)?.as_str().trim();
    if t.is_empty() {
        None
    } else {
        Some(html_unescape(t))
    }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r#"{name}\s*=\s*["']([^"']*)["']"#)).ok()?;
    re.captures(tag)?.get(1).map(|m| m.as_str().to_string())
}

fn guess_title(raw: &str) -> Option<String> {
    for line in raw.lines().take(60) {
        if let Some(rest) = line.trim().strip_prefix("Title:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}
fn guess_author(raw: &str) -> Option<String> {
    for line in raw.lines().take(60) {
        if let Some(rest) = line.trim().strip_prefix("Author:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .replace(['_', '-'], " ")
}

fn titlecase(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "\u{2019}")
        .replace("&nbsp;", " ")
}

/// CBZ (comics) import — F5c reading half. Pages are extracted to
/// `$VENA_ASSET_DIR/manga/<file-stem>/NNNN.<ext>` (served lazily to the UI via
/// `get_manga_page`); each page becomes one "episode" so progress mechanics work
/// unchanged. No prose ⇒ no ledger; the manual-progress companion applies.
pub fn import_cbz(path: &Path) -> Result<ImportedBook> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("comic")
        .to_string();
    let assets = std::env::var("VENA_ASSET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("vena-assets"));
    let slug_dir = assets.join("manga").join(slug_of(&stem));
    std::fs::create_dir_all(&slug_dir)?;

    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    // Collect image entries in name order (standard CBZ page ordering).
    let mut names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_string()))
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            !n.ends_with('/')
                && (l.ends_with(".jpg")
                    || l.ends_with(".jpeg")
                    || l.ends_with(".png")
                    || l.ends_with(".webp")
                    || l.ends_with(".gif"))
        })
        .collect();
    names.sort();
    if names.is_empty() {
        anyhow::bail!("no image pages found in CBZ");
    }

    let mut chapters = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let ext = name
            .rsplit('.')
            .next()
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        let mut entry = zip.by_name(name)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf)?;
        std::fs::write(slug_dir.join(format!("{:04}.{ext}", i + 1)), &buf)?;
        chapters.push(Chapter {
            seq: (i + 1) as i64,
            title: Some(format!("Page {}", i + 1)),
            // Marker paragraph; the UI renders the real page via get_manga_page.
            paragraphs: vec![format!("[comic page {}]", i + 1)],
        });
    }

    Ok(ImportedBook {
        title: stem.replace(['_', '-'], " "),
        author: None,
        chapters,
        cover: None,
        cover_name: None,
        profile: "comic".into(),
        profile_evidence: format!(
            "cbz · {} pages · no prose (companion is manual-progress)",
            names.len()
        ),
    })
}

fn slug_of(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
