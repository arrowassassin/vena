//! Portraits + covers (§11.2, §11.4). Real fallback chain per v2.0:
//!   relay image endpoint (if configured & capable) → local Paint Engine (if
//!   installed) → graceful skip = TYPOGRAPHIC placeholder (initial-letter portrait /
//!   type-set cover), which the spec names as the sanctioned final tier.
//!
//! Rules (§11.4): generate once, cache forever. Portraits are SPOILER-GATED — the
//! prompt is built from the voice card + facts AS OF the reader's current chapter,
//! never beyond. Covers use spoiler-weight-0/1 facts only (title/author/mood/era);
//! never fates or twist imagery.

use crate::api::AppApi;
use vena_core::{Result, VenaError};

impl AppApi {
    /// generate_portrait — cache key includes current progress so a portrait
    /// refreshes when canon changes appearance (per-chapter cache per §11.4).
    /// Emits progress via `on_progress(pct)`; returns the asset path.
    pub fn generate_portrait(
        &self,
        book_id: i64,
        character_id: i64,
        mut on_progress: impl FnMut(u32),
    ) -> Result<String> {
        let (character, progress, gated_texts) = {
            let store = self.store_guard();
            let progress = store.get_progress(book_id)?.0;
            let c = store.get_character(book_id, character_id)?;
            if !c.met {
                return Err(VenaError::NotFound(
                    "keep reading to meet them — portraits are progress-gated".into(),
                ));
            }
            // Appearance facts visible AT CURRENT PROGRESS only (spoiler-gated prompt).
            let facts = store.gated_facts(book_id, progress, None, &c.name, 8)?;
            (
                c.clone(),
                progress,
                facts.into_iter().map(|f| f.text).collect::<Vec<_>>(),
            )
        };

        let dir = self.assets_dir()?;
        let path = dir.join(format!(
            "portrait-{book_id}-{character_id}-ch{progress}.png"
        ));
        let svg_path = dir.join(format!(
            "portrait-{book_id}-{character_id}-ch{progress}.svg"
        ));
        if path.exists() {
            return Ok(path.to_string_lossy().into());
        }
        if svg_path.exists() {
            return Ok(svg_path.to_string_lossy().into());
        }

        on_progress(10);
        // Tier 1: relay image endpoint.
        if let Some(png) = self.relay_image(&portrait_prompt(
            &character.name,
            &character.voice_card.diction,
            &gated_texts,
        ))? {
            std::fs::write(&path, png)?;
            on_progress(100);
            return Ok(path.to_string_lossy().into());
        }
        // Tier 2: local Paint Engine — downloaded GGUF weights rendered through the
        // stable-diffusion.cpp `sd` CLI when it is installed; otherwise fall through.
        on_progress(40);
        if let Some(png_bytes) = self.sd_render(
            &portrait_prompt(&character.name, &character.voice_card.diction, &gated_texts),
            512,
            512,
        )? {
            std::fs::write(&path, png_bytes)?;
            on_progress(100);
            return Ok(path.to_string_lossy().into());
        }
        // Tier 3: graceful skip — initial-letter typographic portrait (real asset).
        on_progress(60);
        let svg = typographic_portrait_svg(&character.name);
        std::fs::write(&svg_path, svg)?;
        on_progress(100);
        Ok(svg_path.to_string_lossy().into())
    }

    /// generate_cover — composed from weight-0/1 facts only (title, author, mood).
    pub fn generate_cover(
        &self,
        book_id: i64,
        regenerate: bool,
        mut on_progress: impl FnMut(u32),
    ) -> Result<String> {
        let (book, ambient) = {
            let store = self.store_guard();
            let b = store.get_book(book_id)?;
            // Cover prompt facts: weight 0/1 AND at-or-before the reader's bookmark.
            // The chapter gate is non-negotiable — this text may be POSTed to a
            // remote image endpoint, and the Cloud Relay invariant forbids ANY
            // ungated/future content leaving the device (§11.4a). At progress 0 the
            // cover falls back to title/author only, which is correct.
            let progress = store.get_progress(book_id)?.0;
            let ambient: Vec<String> = store
                .facts_at_or_before(book_id, progress)?
                .into_iter()
                .filter(|f| f.spoiler_weight <= 1)
                .take(6)
                .map(|f| f.text)
                .collect();
            (b, ambient)
        };
        let dir = self.assets_dir()?;
        let png_path = dir.join(format!("cover-{book_id}.png"));
        let svg_path = dir.join(format!("cover-{book_id}.svg"));
        if !regenerate {
            if png_path.exists() {
                return Ok(png_path.to_string_lossy().into());
            }
            if svg_path.exists() {
                return Ok(svg_path.to_string_lossy().into());
            }
        }

        on_progress(10);
        if let Some(png) =
            self.relay_image(&cover_prompt(&book.title, book.author.as_deref(), &ambient))?
        {
            std::fs::write(&png_path, png)?;
            self.set_cover_asset(book_id, &png_path)?;
            on_progress(100);
            return Ok(png_path.to_string_lossy().into());
        }
        on_progress(40);
        if let Some(png_bytes) = self.sd_render(
            &cover_prompt(&book.title, book.author.as_deref(), &ambient),
            512,
            768,
        )? {
            std::fs::write(&png_path, png_bytes)?;
            self.set_cover_asset(book_id, &png_path)?;
            on_progress(100);
            return Ok(png_path.to_string_lossy().into());
        }
        on_progress(60);
        let svg = typographic_cover_svg(&book.title, book.author.as_deref());
        std::fs::write(&svg_path, svg)?;
        self.set_cover_asset(book_id, &svg_path)?;
        on_progress(100);
        Ok(svg_path.to_string_lossy().into())
    }

    /// Tier 2: render locally via stable-diffusion.cpp's `sd` CLI using downloaded
    /// GGUF weights. None when weights or the engine binary are absent (honest
    /// fallthrough — never blocks on image quality).
    fn sd_render(&self, prompt: &str, w: u32, h: u32) -> Result<Option<Vec<u8>>> {
        let Some((model, engine)) = self.local_paint() else {
            return Ok(None);
        };
        if !engine {
            return Ok(None); // weights downloaded, engine missing — status reports it
        }
        let out = std::env::temp_dir().join(format!("vena-sd-{}.png", std::process::id()));
        let status = std::process::Command::new("sd")
            .args(["-m"])
            .arg(&model)
            .args([
                "-p",
                prompt,
                "--steps",
                "20",
                "-W",
                &w.to_string(),
                "-H",
                &h.to_string(),
                "-o",
            ])
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() && out.exists() => {
                let bytes = std::fs::read(&out)?;
                let _ = std::fs::remove_file(&out);
                Ok(Some(bytes))
            }
            _ => Ok(None),
        }
    }

    /// POST the user's configured image endpoint (OpenAI-compatible
    /// /v1/images/generations, b64 response). None = not configured / not capable.
    fn relay_image(&self, prompt: &str) -> Result<Option<Vec<u8>>> {
        let (base, key, model) = match self.image_config()? {
            Some(c) => c,
            None => return Ok(None),
        };
        let body = serde_json::json!({
            "model": model, "prompt": prompt, "n": 1,
            "size": "832x1216", "response_format": "b64_json",
        });
        let resp = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| VenaError::Other(e.to_string()))?
            .post(format!(
                "{}/v1/images/generations",
                base.trim_end_matches('/')
            ))
            .bearer_auth(&key)
            .json(&body)
            .send();
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            // Not capable / unreachable → fall through the chain, never fail chat.
            _ => return Ok(None),
        };
        let v: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let b64 = v["data"][0]["b64_json"].as_str().map(str::to_string);
        match b64 {
            Some(s) => Ok(Some(base64_decode(&s)?)),
            None => Ok(None),
        }
    }
}

fn portrait_prompt(name: &str, diction: &str, gated: &[String]) -> String {
    format!(
        "Ink-and-paper literary portrait of {name}, 19th-century engraving style, \
         neo-brutalist red/black palette. Character notes: {diction}. Known so far: {}",
        gated.join("; ")
    )
}

fn cover_prompt(title: &str, author: Option<&str>, ambient: &[String]) -> String {
    format!(
        "Book cover, letterpress print aesthetic, cream paper, heavy black keylines, \
         red accent. Title: {title}. Author: {}. Setting mood (no plot events): {}",
        author.unwrap_or("Unknown"),
        ambient.join("; ")
    )
}

/// Initial-letter portrait — the sanctioned typographic tier. Deterministic, real.
fn typographic_portrait_svg(name: &str) -> String {
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let hue = name.bytes().map(u32::from).sum::<u32>() % 360;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
<rect width="512" height="512" fill="hsl({hue} 35% 88%)"/>
<rect x="14" y="14" width="484" height="484" fill="none" stroke="#15151a" stroke-width="8"/>
<text x="256" y="330" font-family="Anton, sans-serif" font-size="300" text-anchor="middle" fill="#15151a">{initial}</text>
<text x="256" y="452" font-family="Oswald, sans-serif" font-size="34" letter-spacing="4" text-anchor="middle" fill="#dd3427">{}</text>
</svg>"##,
        xml_escape(&name.to_uppercase())
    )
}

fn typographic_cover_svg(title: &str, author: Option<&str>) -> String {
    let hue = title.bytes().map(u32::from).sum::<u32>() % 360;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="832" height="1216" viewBox="0 0 832 1216">
<rect width="832" height="1216" fill="hsl({hue} 30% 90%)"/>
<rect x="20" y="20" width="792" height="1176" fill="none" stroke="#15151a" stroke-width="10"/>
<rect x="20" y="980" width="792" height="26" fill="#dd3427"/>
<text x="416" y="480" font-family="Anton, sans-serif" font-size="110" text-anchor="middle" fill="#15151a">{}</text>
<text x="416" y="1120" font-family="Oswald, sans-serif" font-size="44" letter-spacing="6" text-anchor="middle" fill="#15151a">{}</text>
</svg>"##,
        xml_escape(&title.to_uppercase()),
        xml_escape(&author.unwrap_or("").to_uppercase())
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>> {
    // Minimal RFC 4648 decoder — avoids a dependency for one call site.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &b) in TABLE.iter().enumerate() {
        rev[b as usize] = i as u8;
    }
    // Accept the URL-safe alphabet too (- _), which some OpenAI-compatible image
    // relays emit — map them onto the standard values so they decode, not error.
    rev[b'-' as usize] = 62;
    rev[b'_' as usize] = 63;
    let clean: Vec<u8> = s.bytes().filter(|b| !b" \n\r\t".contains(b)).collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let mut chunk = [0u8; 4];
    let mut n = 0;
    for &b in &clean {
        if b == b'=' {
            break;
        }
        let v = rev[b as usize];
        if v == 255 {
            return Err(VenaError::Other("invalid base64 in image response".into()));
        }
        chunk[n] = v;
        n += 1;
        if n == 4 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
            out.push((chunk[2] << 6) | chunk[3]);
            n = 0;
        }
    }
    if n == 3 {
        out.push((chunk[0] << 2) | (chunk[1] >> 4));
        out.push((chunk[1] << 4) | (chunk[2] >> 2));
    } else if n == 2 {
        out.push((chunk[0] << 2) | (chunk[1] >> 4));
    }
    Ok(out)
}
