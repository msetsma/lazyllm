use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::streaming::{check_http_error, stream_sse_response};
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk, TokenUsage};

/// OpenAI-compatible provider. Works with OpenAI API, Azure, and any
/// compatible endpoint (e.g., local servers with OpenAI-compatible API).
#[derive(Debug)]
pub struct OpenAiProvider {
    name: String,
    api_key: String,
    base_url: String,
    models: Vec<ModelInfo>,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(
        name: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            name: name.into(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            models,
            client: Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }
}

/// OpenAI API request body.
#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    stream: bool,
    stream_options: OpenAiStreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

/// SSE stream response chunk.
#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

/// OpenAI error response body.
#[derive(Debug, Deserialize)]
struct OpenAiErrorResponse {
    error: OpenAiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorDetail {
    message: String,
}

impl From<&ChatRequest> for OpenAiRequest {
    fn from(req: &ChatRequest) -> Self {
        let messages = req
            .messages
            .iter()
            .map(|m| OpenAiMessage {
                role: m.role.as_str().to_string(),
                content: m.content.clone(),
            })
            .collect();

        Self {
            model: req.model.clone(),
            messages,
            stream: true,
            stream_options: OpenAiStreamOptions {
                include_usage: true,
            },
            temperature: req.temperature,
            max_tokens: req.max_tokens,
        }
    }
}

/// Parse a single SSE data line into a StreamChunk.
fn parse_sse_line(line: &str) -> Option<StreamChunk> {
    let data = line.strip_prefix("data: ")?;

    if data.trim() == "[DONE]" {
        return Some(StreamChunk::Done);
    }

    let chunk: OpenAiStreamChunk = match serde_json::from_str(data) {
        Ok(c) => c,
        Err(e) => return Some(StreamChunk::Error(format!("Failed to parse chunk: {e}"))),
    };

    // Usage comes in a separate chunk after finish_reason (when stream_options.include_usage is set)
    if let Some(usage) = chunk.usage {
        return Some(StreamChunk::Usage(TokenUsage::new(
            usage.prompt_tokens,
            usage.completion_tokens,
        )));
    }

    if let Some(choice) = chunk.choices.first() {
        if let Some(ref content) = choice.delta.content
            && !content.is_empty()
        {
            return Some(StreamChunk::Delta(content.clone()));
        }
        if choice.finish_reason.as_deref() == Some("stop") {
            return Some(StreamChunk::Done);
        }
    }

    None
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<(), LlmError> {
        let body = OpenAiRequest::from(&request);
        let url = self.chat_url();

        tracing::debug!("Sending chat request to {url}");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let response = check_http_error(response, |body| {
            serde_json::from_str::<OpenAiErrorResponse>(body)
                .map(|e| e.error.message)
                .ok()
        })
        .await?;

        stream_sse_response(response, &tx, parse_sse_line, |_| false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    #[test]
    fn openai_provider_name() {
        let provider = OpenAiProvider::new(
            "openai",
            "test-key",
            "https://api.openai.com/v1",
            vec![ModelInfo::new("gpt-4o")],
        );
        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn openai_provider_models() {
        let provider = OpenAiProvider::new(
            "openai",
            "test-key",
            "https://api.openai.com/v1",
            vec![
                ModelInfo::new("gpt-4o"),
                ModelInfo::new("gpt-4o-mini"),
            ],
        );
        let models = provider.available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-4o");
        assert_eq!(models[1].id, "gpt-4o-mini");
    }

    #[test]
    fn chat_url_construction() {
        let provider = OpenAiProvider::new(
            "openai",
            "key",
            "https://api.openai.com/v1",
            vec![],
        );
        assert_eq!(provider.chat_url(), "https://api.openai.com/v1/chat/completions");

        // With trailing slash
        let provider = OpenAiProvider::new(
            "openai",
            "key",
            "https://api.openai.com/v1/",
            vec![],
        );
        assert_eq!(provider.chat_url(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn openai_request_from_chat_request() {
        let chat_req = ChatRequest::new(
            "gpt-4o",
            vec![
                Message::system("be helpful"),
                Message::user("hello"),
            ],
        )
        .with_temperature(0.7)
        .with_max_tokens(500);

        let openai_req = OpenAiRequest::from(&chat_req);
        assert_eq!(openai_req.model, "gpt-4o");
        assert!(openai_req.stream);
        assert_eq!(openai_req.messages.len(), 2);
        assert_eq!(openai_req.messages[0].role, "system");
        assert_eq!(openai_req.messages[0].content, "be helpful");
        assert_eq!(openai_req.messages[1].role, "user");
        assert_eq!(openai_req.messages[1].content, "hello");
        assert_eq!(openai_req.temperature, Some(0.7));
        assert_eq!(openai_req.max_tokens, Some(500));
    }

    #[test]
    fn openai_request_serializes_correctly() {
        let chat_req = ChatRequest::new("gpt-4o", vec![Message::user("hi")]);
        let openai_req = OpenAiRequest::from(&chat_req);
        let json = serde_json::to_value(&openai_req).unwrap();

        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["stream"], true);
        assert!(json.get("temperature").is_none()); // skipped when None
        assert!(json.get("max_tokens").is_none());
    }

    #[test]
    fn parse_sse_delta_line() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    #[test]
    fn parse_sse_done_line() {
        let line = "data: [DONE]";
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn parse_sse_finish_reason_stop() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn parse_sse_empty_delta_returns_none() {
        // Initial chunk often has role but no content
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let chunk = parse_sse_line(line);
        assert!(chunk.is_none());
    }

    #[test]
    fn parse_sse_non_data_line_returns_none() {
        assert!(parse_sse_line("event: message").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": comment").is_none());
    }

    #[test]
    fn parse_sse_invalid_json_returns_error() {
        let line = "data: {invalid json}";
        let chunk = parse_sse_line(line).unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }

    #[test]
    fn parse_sse_usage_chunk() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[],"usage":{"prompt_tokens":25,"completion_tokens":42,"total_tokens":67}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(
            chunk,
            StreamChunk::Usage(super::super::types::TokenUsage::new(25, 42))
        );
    }
}
