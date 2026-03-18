use lazyllm::llm::ollama::OllamaProvider;
use lazyllm::llm::types::{ChatRequest, LlmError, Message, ModelInfo, StreamChunk};
use lazyllm::llm::LlmProvider;
use mockito::Server;
use tokio::sync::mpsc;

fn make_provider(base_url: &str) -> OllamaProvider {
    OllamaProvider::new(
        "ollama",
        base_url,
        vec![
            ModelInfo::new("llama3.2"),
            ModelInfo::new("mistral"),
        ],
    )
}

/// Ollama streams NDJSON (one JSON object per line, no `data: ` prefix).
fn build_ndjson_body(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|l| format!("{l}\n"))
        .collect::<String>()
}

async fn collect_stream(rx: &mut mpsc::UnboundedReceiver<StreamChunk>) -> String {
    let mut text = String::new();
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(t) => text.push_str(&t),
            StreamChunk::Done => break,
            StreamChunk::Usage(_) => {}
            StreamChunk::Error(e) => panic!("Unexpected error: {e}"),
        }
    }
    text
}

// ── Happy-path streaming ────────────────────────────────────────────

#[tokio::test]
async fn streaming_collects_full_response() {
    let mut server = Server::new_async().await;

    let body = build_ndjson_body(&[
        r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":"Hello"},"done":false}"#,
        r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":" from"},"done":false}"#,
        r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":" Ollama!"},"done":false}"#,
        r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true,"total_duration":1234}"#,
    ]);

    let _mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("llama3.2", vec![Message::user("hello")]);
    provider.chat(request, tx).await.unwrap();

    let text = collect_stream(&mut rx).await;
    assert_eq!(text, "Hello from Ollama!");
}

// ── Multi-turn conversation ─────────────────────────────────────────

#[tokio::test]
async fn multi_turn_sends_correct_body() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"model":"llama3.2","stream":true}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(build_ndjson_body(&[
            r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true}"#,
        ]))
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "llama3.2",
        vec![
            Message::system("You are helpful"),
            Message::user("What is Rust?"),
            Message::assistant("A systems language."),
            Message::user("Tell me more"),
        ],
    );
    provider.chat(request, tx).await.unwrap();
    _mock.assert_async().await;
}

// ── Model-level error (e.g., model not found) ───────────────────────

#[tokio::test]
async fn model_error_in_stream() {
    let mut server = Server::new_async().await;

    let body = build_ndjson_body(&[
        r#"{"error":"model 'nonexistent' not found, try pulling it first"}"#,
    ]);

    let _mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("nonexistent", vec![Message::user("hi")]);
    provider.chat(request, tx).await.unwrap();

    match rx.recv().await.unwrap() {
        StreamChunk::Error(e) => assert!(e.contains("not found")),
        other => panic!("Expected Error, got: {other:?}"),
    }
}

// ── HTTP error (Ollama not running) ─────────────────────────────────

#[tokio::test]
async fn http_error_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/api/chat")
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("llama3.2", vec![Message::user("hello")]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, .. }) => assert_eq!(status, 500),
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Long streaming with receiver drop ───────────────────────────────

#[tokio::test]
async fn receiver_drop_stops_gracefully() {
    let mut server = Server::new_async().await;

    let mut lines = Vec::new();
    for i in 0..100 {
        lines.push(format!(
            r#"{{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{{"role":"assistant","content":"chunk{i} "}},"done":false}}"#
        ));
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let body = build_ndjson_body(&refs);

    let _mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    let request = ChatRequest::new("llama3.2", vec![]);
    let result = provider.chat(request, tx).await;
    assert!(result.is_ok());
}

// ── Empty response (done immediately) ───────────────────────────────

#[tokio::test]
async fn empty_response_sends_done() {
    let mut server = Server::new_async().await;

    let body = build_ndjson_body(&[
        r#"{"model":"llama3.2","created_at":"2024-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true}"#,
    ]);

    let _mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("llama3.2", vec![]);
    provider.chat(request, tx).await.unwrap();

    let chunk = rx.recv().await.unwrap();
    assert_eq!(chunk, StreamChunk::Done);
}
