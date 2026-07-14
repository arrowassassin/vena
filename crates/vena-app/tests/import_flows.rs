//! Real import flows with NO model backend configured: import_book falls back
//! to canon-only (empty ledger), so a .txt and a .cbz import fully on-device.
//! Covers import.rs (text segmentation, CBZ unzip) and the manga read commands.

use std::io::Write;

use vena_app::api::AppApi;
use vena_app::keystore::MemoryKeyStore;

fn fresh_api() -> (AppApi, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let api = AppApi::new(
        dir.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();
    (api, dir)
}

#[test]
fn import_plaintext_book_segments_chapters() {
    let (api, dir) = fresh_api();
    let para = vec!["word"; 200].join(" ");
    let text =
        format!("The Test Novel\nby A. Writer\n\nCHAPTER I\n\n{para}\n\nCHAPTER II\n\n{para}\n");
    let path = dir.path().join("test-novel.txt");
    std::fs::write(&path, text).unwrap();

    let meta = api.import_book(&path.to_string_lossy(), |_, _| {}).unwrap();
    assert!(meta.episode_count >= 2, "chapters segmented: {meta:?}");
    assert_eq!(meta.profile, "prose");
    // it's on the shelf and readable
    let ep = api.get_episode(meta.id, 1).unwrap();
    assert!(ep.content_html.contains("word"));
}

/// Build a minimal real CBZ (zip of PNG-ish files) in a temp path.
fn make_cbz(path: &std::path::Path, pages: usize) {
    let f = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // 1x1 PNG bytes (valid header), one per page
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    for i in 0..pages {
        zip.start_file(format!("page-{i:03}.png"), opts).unwrap();
        zip.write_all(png).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn import_cbz_makes_comic_and_serves_pages() {
    let (api, dir) = fresh_api();
    let cbz = dir.path().join("My Comic.cbz");
    make_cbz(&cbz, 4);

    let meta = api.import_book(&cbz.to_string_lossy(), |_, _| {}).unwrap();
    assert_eq!(meta.profile, "comic");
    assert_eq!(meta.episode_count, 4, "one episode per page");

    let pages = api.get_manga_pages(meta.id).unwrap();
    assert_eq!(pages["count"].as_i64().unwrap(), 4);
    assert_eq!(pages["profile"], "comic");

    // a real page comes back as base64 image data
    let page = api.get_manga_page(meta.id, 1).unwrap();
    let data = page["data"].as_str().unwrap();
    assert!(!data.is_empty());
    // out-of-range page is an error, not a panic
    assert!(api.get_manga_page(meta.id, 99).is_err());
}

#[test]
fn import_book_data_neutralizes_path_escapes() {
    let (api, dir) = fresh_api();
    // "../evil.txt" is sanitized to its final component ("evil.txt") and written
    // INSIDE imports/, never outside — so no file escapes the profile dir.
    let _ = api.import_book_data("../../evil.txt", "aGk=", |_, _| {});
    assert!(
        !dir.path().parent().unwrap().join("evil.txt").exists(),
        "no file written outside the profile dir"
    );
    // a name that is ONLY traversal has no file component → hard error
    assert!(api.import_book_data("..", "aGk=", |_, _| {}).is_err());
}

#[test]
fn burn_reclaims_comic_pages_from_disk() {
    let (api, dir) = fresh_api();
    let cbz = dir.path().join("Burn Me.cbz");
    make_cbz(&cbz, 3);
    let meta = api.import_book(&cbz.to_string_lossy(), |_, _| {}).unwrap();
    let manga_dir = dir.path().join("assets").join("manga").join(&meta.slug);
    assert!(manga_dir.exists(), "pages extracted to disk");
    api.delete_book(meta.id).unwrap();
    assert!(!manga_dir.exists(), "burn removed the extracted pages");
}
