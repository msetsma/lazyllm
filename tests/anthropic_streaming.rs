use lazyllm::llm::anthropic::AnthropicProvider;
use lazyllm::llm::types::{ChatRequest, LlmError, Message, ModelInfo, StreamChunk};
use lazyllm::llm::LlmProvider;
use mockito::Server;
use tokio::sync::mpsc;

/// Build an SSE body from raw lines (each gets `data: ...\n\n` wrapped).
fn build_sse_body(chunks: &[&str]) -> String {
    chunks
        .iter()
        .map(|c| format!("data: {c}\n\n"))
        .collect::<String>()
}

fn make_provider(base_url: &str) -> AnthropicProvider {
    AnthropicProvider::new(
        "anthropic",
        "test-key",
        // The provider normalises the URL, so pass the mock server URL directly.
        // It will append /v1/messages itself.
        base_url,
        vec![
            ModelInfo::new("claude-sonnet-4-20250514"),
            ModelInfo::new("claude-haiku-4-5-20251001"),
        ],
    )
}

/// Collect all stream chunks into a single string, panic on errors.
async fn collect_stream(rx: &mut mpsc::UnboundedReceiver<StreamChunk>) -> String {
    let mut text = String::new();
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(t) => text.push_str(&t),
            StreamChunk::Done => break,
            StreamChunk::Usage(_) => {}
            StreamChunk::Error(e) => panic!("Unexpected error: {e}"),
            _ => {}
        }
    }
    text
}

// ── Happy-path streaming ────────────────────────────────────────────

#[tokio::test]
async fn streaming_collects_full_response() {
    let mut server = Server::new_async().await;

    let body = build_sse_body(&[
        r#"{"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[]}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" from"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" Claude!"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        r#"{"type":"message_stop"}"#,
    ]);

    let _mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-version", "2023-06-01")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "claude-sonnet-4-20250514",
        vec![Message::user("hello")],
    );
    provider.chat(request, tx).await.unwrap();

    let text = collect_stream(&mut rx).await;
    assert_eq!(text, "Hello from Claude!");
}

// ── Multi-turn conversation ─────────────────────────────────────────

#[tokio::test]
async fn multi_turn_conversation_sends_correct_body() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"model":"claude-sonnet-4-20250514","stream":true}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(build_sse_body(&[r#"{"type":"message_stop"}"#]))
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "claude-sonnet-4-20250514",
        vec![
            Message::system("You are helpful"),
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
            Message::user("Tell me more"),
        ],
    );
    provider.chat(request, tx).await.unwrap();
    _mock.assert_async().await;
}

// ── Auth error ──────────────────────────────────────────────────────

#[tokio::test]
async fn auth_error_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_body(r#"{"type":"error","error":{"type":"authentication_error","message":"Invalid API key"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "claude-sonnet-4-20250514",
        vec![Message::user("hello")],
    );
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 401);
            assert!(message.contains("Invalid API key"));
        }
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Rate limit ──────────────────────────────────────────────────────

#[tokio::test]
async fn rate_limit_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(429)
        .with_body(r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limit exceeded"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("claude-sonnet-4-20250514", vec![]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 429);
            assert!(message.contains("Rate limit"));
        }
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Overloaded (529) ────────────────────────────────────────────────

#[tokio::test]
async fn overloaded_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(529)
        .with_body(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("claude-sonnet-4-20250514", vec![]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 529);
            assert!(message.contains("Overloaded"));
        }
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Streaming error event mid-stream ────────────────────────────────

#[tokio::test]
async fn streaming_error_event_is_propagated() {
    let mut server = Server::new_async().await;

    let body = build_sse_body(&[
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}"#,
        r#"{"type":"error","error":{"message":"internal server error"}}"#,
    ]);

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);
    provider.chat(request, tx).await.unwrap();

    let mut got_delta = false;
    let mut got_error = false;
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(t) => {
                assert_eq!(t, "partial");
                got_delta = true;
            }
            StreamChunk::Error(e) => {
                assert!(e.contains("internal server error"));
                got_error = true;
                break;
            }
            StreamChunk::Usage(_) => {}
            StreamChunk::Done => break,
            _ => {}
        }
    }
    assert!(got_delta, "should have received partial delta");
    assert!(got_error, "should have received error event");
}

// ── Receiver drop ───────────────────────────────────────────────────

#[tokio::test]
async fn receiver_drop_stops_gracefully() {
    let mut server = Server::new_async().await;

    let mut lines = Vec::new();
    for i in 0..50 {
        lines.push(format!(
            r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"chunk{i} "}}}}"#
        ));
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let body = build_sse_body(&refs);

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    let request = ChatRequest::new("claude-sonnet-4-20250514", vec![]);
    let result = provider.chat(request, tx).await;
    assert!(result.is_ok());
}
