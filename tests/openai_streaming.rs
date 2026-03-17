use lazyllm::llm::openai::OpenAiProvider;
use lazyllm::llm::types::{ChatRequest, LlmError, Message, ModelInfo, StreamChunk};
use lazyllm::llm::LlmProvider;
use mockito::Server;
use tokio::sync::mpsc;

/// Helper to build an SSE response body from chunks.
fn build_sse_body(chunks: &[&str]) -> String {
    chunks
        .iter()
        .map(|c| format!("data: {c}\n\n"))
        .collect::<String>()
}

fn sse_delta(content: &str) -> String {
    format!(
        r#"{{"id":"chatcmpl-test","choices":[{{"index":0,"delta":{{"content":"{content}"}},"finish_reason":null}}]}}"#
    )
}

fn sse_done_choice() -> &'static str {
    r#"{"id":"chatcmpl-test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#
}

fn make_provider(base_url: &str) -> OpenAiProvider {
    OpenAiProvider::new(
        "test-openai",
        "test-key",
        base_url,
        vec![ModelInfo::new("gpt-4o")],
    )
}

#[tokio::test]
async fn streaming_collects_full_response() {
    let mut server = Server::new_async().await;

    let sse_body = build_sse_body(&[
        &sse_delta("Hello"),
        &sse_delta(" world"),
        &sse_delta("!"),
        sse_done_choice(),
        "[DONE]",
    ]);

    let _mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gpt-4o", vec![Message::user("hello")]);
    provider.chat(request, tx).await.unwrap();

    // Collect all chunks
    let mut full_text = String::new();
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(text) => full_text.push_str(&text),
            StreamChunk::Done => break,
            StreamChunk::Error(e) => panic!("Unexpected error: {e}"),
        }
    }

    assert_eq!(full_text, "Hello world!");
}

#[tokio::test]
async fn auth_error_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(401)
        .with_body(r#"{"error":{"message":"Incorrect API key provided","type":"auth_error"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gpt-4o", vec![Message::user("hello")]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 401);
            assert!(message.contains("Incorrect API key"));
        }
        other => panic!("Expected ApiError, got: {:?}", other),
    }
}

#[tokio::test]
async fn rate_limit_returns_api_error() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_body(r#"{"error":{"message":"Rate limit exceeded","type":"rate_limit_error"}}"#)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gpt-4o", vec![]);
    let result = provider.chat(request, tx).await;

    match result {
        Err(LlmError::ApiError { status, message }) => {
            assert_eq!(status, 429);
            assert!(message.contains("Rate limit"));
        }
        other => panic!("Expected ApiError, got: {:?}", other),
    }
}

#[tokio::test]
async fn streaming_with_only_done_signal() {
    let mut server = Server::new_async().await;

    let sse_body = build_sse_body(&["[DONE]"]);

    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new("gpt-4o", vec![]);
    provider.chat(request, tx).await.unwrap();

    // Should get Done immediately
    let chunk = rx.recv().await.unwrap();
    assert_eq!(chunk, StreamChunk::Done);
}

#[tokio::test]
async fn provider_sends_correct_request_body() {
    let mut server = Server::new_async().await;

    let _mock = server
        .mock("POST", "/chat/completions")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"model":"gpt-4o","stream":true}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body("data: [DONE]\n\n")
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, _rx) = mpsc::unbounded_channel();

    let request = ChatRequest::new(
        "gpt-4o",
        vec![Message::system("be helpful"), Message::user("hi")],
    )
    .with_temperature(0.5)
    .with_max_tokens(100);

    provider.chat(request, tx).await.unwrap();
    _mock.assert_async().await;
}

#[tokio::test]
async fn receiver_drop_stops_gracefully() {
    let mut server = Server::new_async().await;

    // Send a large streaming response
    let mut chunks = Vec::new();
    for i in 0..100 {
        chunks.push(sse_delta(&format!("chunk{i} ")));
    }
    let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
    let sse_body = build_sse_body(&refs);

    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = make_provider(&server.url());
    let (tx, rx) = mpsc::unbounded_channel();

    // Drop receiver immediately
    drop(rx);

    let request = ChatRequest::new("gpt-4o", vec![]);
    // Should return Ok even though receiver is dropped
    let result = provider.chat(request, tx).await;
    assert!(result.is_ok());
}
