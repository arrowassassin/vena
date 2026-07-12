//! Small cross-crate helpers with one canonical definition — the slug is a
//! cross-artifact identity key (forge writes it into .vena, pkg de-dupes on import,
//! the app generates it on EPUB import, the UI keys covers/state on it), so it must
//! be computed identically everywhere.

use crate::store::Store;
use crate::Result;

/// Lowercase, alphanumerics kept, every other run collapsed to a single '-'.
pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// First free `slug`, `slug-2`, `slug-3`, … in the given profile store.
pub fn unique_slug(store: &Store, slug: &str) -> Result<String> {
    let mut candidate = slug.to_string();
    let mut n = 1;
    while store.slug_exists(&candidate)? {
        n += 1;
        candidate = format!("{slug}-{n}");
    }
    Ok(candidate)
}
