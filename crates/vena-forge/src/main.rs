//! vena-forge CLI. Two real forge paths:
//!   forge  --input book.epub                 (full-tier: model via VENA_BASE_URL/KEY/MODEL)
//!   forge  --input book.txt --curated l.json (maintainer prebuilt: authored ledger)
//! plus `inspect` (the import-inspection preview) and `import` (round-trip check).

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use vena_core::inference::OpenAiClient;
use vena_core::pkg;
use vena_core::store::Store;
use vena_forge::{forge, import, ledger};

#[derive(Parser)]
#[command(name = "vena-forge", about = "Forge a book into a .vena package")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the import inspection: detected profile + chapter breakdown.
    Inspect {
        #[arg(long)]
        input: PathBuf,
    },
    /// Forge a book into a .vena package.
    Forge {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Maintainer-curated ledger JSON (prebuilt path). Omit to forge with a model.
        #[arg(long)]
        curated: Option<PathBuf>,
        #[arg(long)]
        slug: Option<String>,
        #[arg(long, default_value = "public-domain")]
        license: String,
        #[arg(long)]
        source: Option<String>,
    },
    /// Import a .vena into a throwaway profile and print the gate stats (round-trip test).
    Import {
        #[arg(long)]
        vena: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Inspect { input } => {
            let book = import::import_path(&input)?;
            println!("TITLE   {}", book.title);
            println!("AUTHOR  {}", book.author.as_deref().unwrap_or("—"));
            println!(
                "PROFILE {} · {}",
                book.profile.to_uppercase(),
                book.profile_evidence
            );
            println!("CHAPTERS {}", book.chapters.len());
            for ch in book.chapters.iter().take(6) {
                println!(
                    "  {:>3}. {:<32} {:>5} words · {} min",
                    ch.seq,
                    ch.title
                        .clone()
                        .unwrap_or_else(|| format!("Chapter {}", ch.seq)),
                    ch.word_count(),
                    ch.est_minutes()
                );
            }
            if book.chapters.len() > 6 {
                println!("  … {} more", book.chapters.len() - 6);
            }
        }

        Cmd::Forge {
            input,
            out,
            curated,
            slug,
            license,
            source,
        } => {
            let book = import::import_path(&input)?;
            forge::require_nonempty(&book)?;
            println!(
                "IMPORTED  {} · {} chapters · {}",
                book.title,
                book.chapters.len(),
                book.profile_evidence
            );

            let (slug, license, led) = if let Some(cpath) = curated {
                let json = std::fs::read_to_string(&cpath)
                    .with_context(|| format!("reading {}", cpath.display()))?;
                let (c, led) = ledger::load_curated(&json)?;
                println!(
                    "LEDGER    curated · {} characters · {} facts · {} edges",
                    led.characters.len(),
                    led.facts.len(),
                    led.edges.len()
                );
                (slug.unwrap_or(c.slug), c.license, led)
            } else {
                let backend = cloud_relay_from_env().context(
                    "no --curated ledger and no VENA_BASE_URL/KEY/MODEL set for model forge",
                )?;
                println!("LEDGER    forging with {} …", backend.0);
                let led = ledger::extract_with_model(
                    backend.1.as_ref(),
                    &book.chapters,
                    |seq, total| {
                        println!("  chapter {seq}/{total} forged");
                    },
                )?;
                println!(
                    "LEDGER    model · {} characters · {} facts · {} edges",
                    led.characters.len(),
                    led.facts.len(),
                    led.edges.len()
                );
                (slug.unwrap_or_else(|| slugify(&book.title)), license, led)
            };

            // Build the package db in a temp dir, write cover if present.
            let tmp = tempdir()?;
            let db_path = tmp.join("package.db");
            let cover_asset = None;
            let stats = forge::forge_to_db(
                &book,
                &led,
                &slug,
                &license,
                source.as_deref(),
                cover_asset,
                &db_path,
            )?;

            let cover = book
                .cover
                .as_ref()
                .zip(book.cover_name.as_deref())
                .map(|(bytes, name)| (name, bytes.as_slice()));
            let sha = pkg::write_vena(&db_path, cover, &out)?;

            println!("\nFORGED ✓  {}", out.display());
            println!(
                "  {} chapters · {} scenes · {} characters · {} facts · {} edges",
                stats.chapters, stats.scenes, stats.characters, stats.facts, stats.edges
            );
            println!(
                "  COVERAGE {}%",
                (stats.ledger_coverage * 100.0).round() as i64
            );
            println!(
                "  CONTENT SHA {}…{}",
                &stats.content_sha[..8],
                &stats.content_sha[stats.content_sha.len() - 2..]
            );
            println!("  PACKAGE SHA {}…{}", &sha[..8], &sha[sha.len() - 2..]);
        }

        Cmd::Import { vena } => {
            let profile = Store::in_memory()?;
            let sid = pkg::import_vena(&profile, &vena)?;
            let book = profile.get_book(sid)?;
            println!("IMPORTED  {} (story_id {})", book.title, sid);
            println!(
                "  {} chapters · {} facts · coverage {}%",
                book.episode_count,
                book.fact_count,
                (book.ledger_coverage * 100.0).round() as i64
            );
            // Prove the gate works post-import at ch 6.
            profile.set_progress(sid, 6, 0)?;
            let visible = profile.gated_facts(sid, 6, None, "", 9999)?;
            let forbidden = profile.forbidden_facts(sid, 6, None)?;
            println!(
                "  at ch.6: {} facts visible, {} sealed (future)",
                visible.len(),
                forbidden.len()
            );
        }
    }
    Ok(())
}

fn cloud_relay_from_env() -> Option<(String, Box<dyn vena_core::inference::Inference>)> {
    let base = std::env::var("VENA_BASE_URL").ok()?;
    let key = std::env::var("VENA_API_KEY").unwrap_or_default();
    let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    let client = OpenAiClient::new(&base, &key, &model);
    Some((format!("{base} ({model})"), Box::new(client)))
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn tempdir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("vena-forge-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
