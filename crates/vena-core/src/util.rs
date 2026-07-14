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

/// The lowercased host of a URL, robust against the authority tricks a byte
/// split would miss. Strips scheme, any `userinfo@` (so
/// `https://good.com:x@evil.com/` resolves to `evil.com`, the real host, not
/// `good.com`), path, query, fragment, and the port. Used by the network
/// allowlist and the remote/loopback classifier — one parser so they cannot
/// disagree about what host a URL actually targets.
pub fn url_host(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    // authority ends at the first '/', '?' or '#'
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    // strip userinfo — everything up to and including the LAST '@'
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // strip the port. Bracketed IPv6 (`[::1]:443`) keeps its brackets' contents.
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        hostport.split(':').next().unwrap_or(hostport)
    };
    host.to_ascii_lowercase()
}

/// Whether a host is loopback (on this device). Single source of truth for the
/// Cloud Relay "nothing ungated leaves the device" invariant.
pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]"
    ) || host.ends_with(".localhost")
}

#[cfg(test)]
mod url_tests {
    use super::*;
    #[test]
    fn userinfo_authority_does_not_spoof_host() {
        assert_eq!(url_host("https://huggingface.co:x@evil.com/f"), "evil.com");
        assert_eq!(url_host("http://localhost:pw@evil.com/v1"), "evil.com");
        assert_eq!(
            url_host("https://gutenberg.org/ebooks/1.epub"),
            "gutenberg.org"
        );
        assert_eq!(url_host("http://127.0.0.1:11434/v1"), "127.0.0.1");
        assert_eq!(url_host("http://[::1]:8080/x"), "::1");
    }
    #[test]
    fn loopback_classification() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(!is_loopback_host("evil.com"));
    }
}
