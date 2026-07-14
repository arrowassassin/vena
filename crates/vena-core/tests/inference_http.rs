//! Real HTTP round-trips for both Cloud Relay clients (OpenAI-compatible and
//! native Anthropic) against a loopback stub — no mocks, real serialization and
//! response parsing, including the error and empty-content branches.

use vena_core::inference::{AnthropicClient, GenOptions, Inference, OpenAiClient};

/// A loopback server that speaks both protocols. It routes on the request path
/// and returns error/empty responses when the prompt asks for them, so every
/// branch of the clients is exercised for real.
fn spawn() -> (u16, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let handle = std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let path = req.url().to_string();
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            let (code, json): (u16, String) = if body.contains("BOOM") {
                (500, "{}".to_string())
            } else if path.contains("/v1/messages") {
                if body.contains("EMPTY") {
                    (200, r#"{"content":[]}"#.to_string())
                } else {
                    (
                        200,
                        r#"{"content":[{"text":"anthropic says hello"}]}"#.to_string(),
                    )
                }
            } else if body.contains("EMPTY") {
                (200, r#"{"choices":[{"message":{}}]}"#.to_string())
            } else {
                (
                    200,
                    r#"{"choices":[{"message":{"content":"openai says hello"}}]}"#.to_string(),
                )
            };
            let resp = tiny_http::Response::from_string(json).with_status_code(code);
            let _ = req.respond(resp);
        }
    });
    (port, handle)
}

#[test]
fn openai_client_completes_and_reports_identity() {
    let (port, _h) = spawn();
    let c = OpenAiClient::new(&format!("http://127.0.0.1:{port}/v1"), "key", "gpt-x");
    // loopback ⇒ on-device (not remote)
    assert!(!c.is_remote());
    assert!(c.name().contains("gpt-x"));
    let out = c
        .complete("system", "user", &GenOptions::default())
        .unwrap();
    assert_eq!(out, "openai says hello");
    // json mode still round-trips (sets response_format on the wire)
    let opts = GenOptions {
        max_tokens: 32,
        temperature: 0.0,
        json: true,
    };
    assert_eq!(c.complete("s", "u", &opts).unwrap(), "openai says hello");
}

#[test]
fn openai_client_surfaces_http_errors_and_empty_content() {
    let (port, _h) = spawn();
    let c = OpenAiClient::new(&format!("http://127.0.0.1:{port}"), "key", "m");
    // a 5xx becomes an Inference error mentioning the status
    let err = c
        .complete("s", "please BOOM", &GenOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("backend returned"), "{err}");
    // a 200 with no content field is also an error, not a panic
    assert!(c.complete("s", "EMPTY", &GenOptions::default()).is_err());
}

#[test]
fn anthropic_client_completes_and_is_always_remote() {
    let (port, _h) = spawn();
    let c = AnthropicClient::new(&format!("http://127.0.0.1:{port}"), "key", "claude-x");
    assert!(c.is_remote());
    assert!(c.name().contains("claude-x"));
    let out = c
        .complete("system", "user", &GenOptions::default())
        .unwrap();
    assert_eq!(out, "anthropic says hello");
    // json mode appends the strict-JSON instruction and still parses
    let opts = GenOptions {
        max_tokens: 16,
        temperature: 0.0,
        json: true,
    };
    assert_eq!(c.complete("s", "u", &opts).unwrap(), "anthropic says hello");
}

#[test]
fn anthropic_client_surfaces_errors_and_empty_content() {
    let (port, _h) = spawn();
    let c = AnthropicClient::new(&format!("http://127.0.0.1:{port}/v1"), "key", "m");
    assert!(c
        .complete("s", "BOOM please", &GenOptions::default())
        .is_err());
    assert!(c.complete("s", "EMPTY", &GenOptions::default()).is_err());
}

#[test]
fn chat_default_flatten_reaches_the_backend() {
    let (port, _h) = spawn();
    let c = OpenAiClient::new(&format!("http://127.0.0.1:{port}/v1"), "k", "m");
    let history = vec![
        ("user".to_string(), "earlier question".to_string()),
        ("assistant".to_string(), "earlier answer".to_string()),
    ];
    // OpenAiClient doesn't override chat(); the trait default flattens history
    // into the system prompt and calls complete() — a real round-trip.
    let out = c
        .chat("system", &history, "now", &GenOptions::default())
        .unwrap();
    assert_eq!(out, "openai says hello");
}
