use lazyllm::llm::google::GoogleProvider;
use lazyllm::llm::types::{ChatRequest, LlmError, Message, ModelInfo, StreamChunk};
use lazyllm::llm::LlmProvider;
use mockito::Server;
use tokio::sync::mpsc;

fn build_sse_body(chunks: &[&str]) -> String {
    chunks
        .iter()
        .map(|c| format!("data: {c}\n\n"))
        .collect::<String>()
}

fn make_provider(base_url: &str) -> GoogleProvider {
    GoogleProvider::new(
        "google",
        "test-api-key",
        base_url,
        vec![
            ModelInfo::new("gemini-2.0-flash"),
            ModelInfo::new("gemini-2.5-pro"),
        ],
    )
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

    let body = build_sse_body(&[
        r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"},"index":0}]}"#,
        r#"{"candidates":[{"content":{"parts":[{"text":" from"}],"role":"model"},"index":0}]}"#,
        r#"{"candidates":[{"content":{"parts":[{"text":" Gemini!"}],"role":"model"},"index":0}]}"#,
        r#"{"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP","index":0}]}"#,
    ]);

    // Gemini URL pattern: /models/{model}:streamGenerateContent?alt=sse&key=...
    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(
                r"^/models/gemini-2\.0-flash:streamGenerateContent".to_string(),
            ),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "gemini-2.0-flash",
        vec![Message::user("hello")],
    );
    provider.chat(request, tx).await.unwrap();

    let text = collect_stream(&mut rx).await;
    assert_eq!(text, "Hello from Gemini!");
}

// ── Multi-turn with system instruction ──────────────────────────────

#[tokio::test]
async fn multi_turn_with_system_instruction() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/gemini-2\.5-pro:streamGenerateContent".to_string()),
        )
        .match_body(mockito::Matcher::AllOf(vec![
            // System instruction is sent as systemInstruction (camelCase)
            mockito::Matcher::PartialJsonString(
                r#"{"systemInstruction":{"role":"user","parts":[{"text":"Be concise"}]}}"#.to_string(),
            ),
            // assistant -> model role mapping
            mockito::Matcher::PartialJsonString(
                r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]},{"role":"model","parts":[{"text":"hello"}]},{"role":"user","parts":[{"text":"more"}]}]}"#.to_string(),
            ),
        ]))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(build_sse_body(&[
            r#"{"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP","index":0}]}"#,
        ]))
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "gemini-2.5-pro",
        vec![
            Message::system("Be concise"),
            Message::user("hi"),
            Message::assistant("hello"),
            Message::user("more"),
        ],
    );
    provider.chat(request, tx).await.unwrap();
    _mock.assert_async().await;
}

// ── Auth error (invalid API key) ────────────────────────────────────

#[tokio::test]
async fn auth_error_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/".to_string()),
        )
        .with_status(400)
        .with_body(r#"{"error":{"code":400,"message":"API key not valid","status":"INVALID_ARGUMENT"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gemini-2.0-flash", vec![Message::user("hello")]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 400);
            assert!(message.contains("API key not valid"));
        }
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Quota exceeded ──────────────────────────────────────────────────

#[tokio::test]
async fn quota_exceeded_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/".to_string()),
        )
        .with_status(429)
        .with_body(r#"{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gemini-2.0-flash", vec![]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 429);
            assert!(message.contains("Quota exceeded"));
        }
        other => panic!("Expected ApiError, got: {other:?}"),
    }
}

// ── Streaming error mid-stream ──────────────────────────────────────

#[tokio::test]
async fn streaming_error_is_propagated() {
    let mut server = Server::new_async().await;

    let body = build_sse_body(&[
        r#"{"candidates":[{"content":{"parts":[{"text":"partial"}],"role":"model"},"index":0}]}"#,
        r#"{"error":{"message":"content filter triggered"}}"#,
    ]);

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gemini-2.0-flash", vec![Message::user("hi")]);
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
                assert!(e.contains("content filter"));
                got_error = true;
                break;
            }
            StreamChunk::Usage(_) => {}
            StreamChunk::Done => break,
        }
    }
    assert!(got_delta);
    assert!(got_error);
}

// ── Receiver drop ───────────────────────────────────────────────────

#[tokio::test]
async fn receiver_drop_stops_gracefully() {
    let mut server = Server::new_async().await;

    let mut lines = Vec::new();
    for i in 0..50 {
        lines.push(format!(
            r#"{{"candidates":[{{"content":{{"parts":[{{"text":"chunk{i} "}}],"role":"model"}},"index":0}}]}}"#
        ));
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let body = build_sse_body(&refs);

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    let request = ChatRequest::new("gemini-2.0-flash", vec![]);
    let result = provider.chat(request, tx).await;
    assert!(result.is_ok());
}

// ── Empty response ──────────────────────────────────────────────────

#[tokio::test]
async fn empty_response_with_stop() {
    let mut server = Server::new_async().await;

    let body = build_sse_body(&[
        r#"{"candidates":[{"content":{"parts":[{"text":""}],"role":"model"},"finishReason":"STOP","index":0}]}"#,
    ]);

    let _mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/models/".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gemini-2.0-flash", vec![]);
    provider.chat(request, tx).await.unwrap();

    let chunk = rx.recv().await.unwrap();
    assert_eq!(chunk, StreamChunk::Done);
}
