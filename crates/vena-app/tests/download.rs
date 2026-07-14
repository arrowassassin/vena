//! Exercises the resumable download machinery in net.rs against a real local
//! HTTP server (loopback is allowlisted). Covers: fresh download + SHA verify,
//! resume from a .part, SHA mismatch discard, and allowlist rejection.

use std::io::Write;

use vena_app::net;

/// A tiny HTTP file server on loopback that supports Range requests, so the
/// resume path is genuinely exercised.
struct FileServer {
    port: u16,
    _thread: std::thread::JoinHandle<()>,
}

fn serve(body: Vec<u8>) -> FileServer {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let thread = std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let range = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Range"))
                .and_then(|h| {
                    h.value
                        .as_str()
                        .strip_prefix("bytes=")
                        .and_then(|r| r.split('-').next())
                        .and_then(|n| n.parse::<usize>().ok())
                });
            let (start, code) = match range {
                Some(s) if s >= body.len() => (s, 416),
                Some(s) => (s, 206),
                None => (0, 200),
            };
            if code == 416 {
                let _ = req.respond(tiny_http::Response::from_string("").with_status_code(416));
                continue;
            }
            let slice = body[start.min(body.len())..].to_vec();
            let _ = req.respond(tiny_http::Response::from_data(slice).with_status_code(code));
        }
    });
    FileServer {
        port,
        _thread: thread,
    }
}

fn sha256(bytes: &[u8]) -> String {
    vena_core::hash::sha256_hex(bytes)
}

#[test]
fn fresh_download_verifies_and_renames() {
    let body = b"hello vena download body".repeat(1000);
    let srv = serve(body.clone());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    let url = format!("http://127.0.0.1:{}/f.bin", srv.port);
    let mut last = 0;
    net::download_file_verified(&url, &dest, Some(&sha256(&body)), &[], &mut |p| last = p).unwrap();
    assert_eq!(last, 100);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert!(!dest.with_extension("part").exists(), ".part cleaned up");
}

#[test]
fn resume_from_partial_completes() {
    let body = b"resume me from the middle please".repeat(500);
    let srv = serve(body.clone());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("r.bin");
    // pre-seed a .part with the first 4000 bytes (as if a prior run stopped)
    let mut part = std::fs::File::create(dest.with_extension("part")).unwrap();
    part.write_all(&body[..4000]).unwrap();
    drop(part);
    let url = format!("http://127.0.0.1:{}/r.bin", srv.port);
    net::download_file_verified(&url, &dest, Some(&sha256(&body)), &[], &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[test]
fn sha_mismatch_is_discarded() {
    let body = b"body that will fail its digest".to_vec();
    let srv = serve(body);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("bad.bin");
    let url = format!("http://127.0.0.1:{}/bad.bin", srv.port);
    let wrong = "0".repeat(64);
    let err = net::download_file_verified(&url, &dest, Some(&wrong), &[], &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("SHA-256 mismatch"), "{err}");
    assert!(!dest.exists());
    assert!(!dest.with_extension("part").exists(), "bad .part removed");
}

#[test]
fn disallowed_host_is_refused_before_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("x.bin");
    let err = net::download_file_verified(
        "https://evil.example.com/x.bin",
        &dest,
        None,
        &[],
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(err.code(), "NetworkNotAllowed");
}

#[test]
fn download_file_plain_wrapper_works() {
    let body = b"no digest supplied here".to_vec();
    let srv = serve(body.clone());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("plain.bin");
    let url = format!("http://127.0.0.1:{}/plain.bin", srv.port);
    net::download_file(&url, &dest, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

/// A loopback server that always answers with a fixed status code (no body).
fn serve_status(code: u16) -> FileServer {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let thread = std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let _ = req.respond(tiny_http::Response::from_string("").with_status_code(code));
        }
    });
    FileServer {
        port,
        _thread: thread,
    }
}

#[test]
fn http_error_status_surfaces_with_a_hint() {
    let srv = serve_status(401);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("gated.bin");
    let url = format!("http://127.0.0.1:{}/gated.bin", srv.port);
    let err = net::download_file(&url, &dest, &mut |_| {}).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("401"), "{msg}");
    // the 401/404 hint about the HF path is attached
    assert!(msg.contains("expected") || msg.contains("moved"), "{msg}");
}

#[test]
fn oversized_part_without_sha_is_discarded() {
    // .part already holds the whole body; a resume Range gets 416. With NO digest
    // to trust the bytes, the client must discard and error rather than rename.
    let body = b"exactly these bytes".repeat(10);
    let srv = serve(body.clone());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("full.bin");
    std::fs::write(dest.with_extension("part"), &body).unwrap();
    let url = format!("http://127.0.0.1:{}/full.bin", srv.port);
    let err = net::download_file(&url, &dest, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("restarting"), "{err}");
    assert!(!dest.with_extension("part").exists(), "bad .part discarded");
}

#[test]
fn probe_openai_base_true_for_live_server_false_for_dead() {
    // the file server answers 200 on any path, so /v1/models resolves → live
    let srv = serve(b"{\"data\":[]}".to_vec());
    let base = format!("http://127.0.0.1:{}/v1", srv.port);
    assert!(net::probe_openai_base(&base));
    // nothing is listening on this port
    assert!(!net::probe_openai_base("http://127.0.0.1:1/v1"));
}
