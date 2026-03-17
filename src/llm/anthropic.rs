use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic Messages API provider with SSE streaming.
#[derive(Debug)]
pub struct AnthropicProvider {
    name: String,
    api_key: String,
    base_url: String,
    models: Vec<ModelInfo>,
    client: Client,
}

impl AnthropicProvider {
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

    pub fn from_env(
        name: impl Into<String>,
        api_key_env: &str,
        models: Vec<String>,
    ) -> Result<Self, LlmError> {
        let api_key = std::env::var(api_key_env).map_err(|_| {
            LlmError::AuthError(format!("Environment variable {api_key_env} not set"))
        })?;

        let model_infos = models.into_iter().map(ModelInfo::new).collect();
        Ok(Self::new(name, api_key, ANTHROPIC_API_URL, model_infos))
    }

    fn messages_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1/messages") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }
}

/// Anthropic API request body.
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

/// SSE event types from the Anthropic API.
#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    error: Option<AnthropicErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    #[serde(default)]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    message: String,
}

/// Anthropic error response body.
#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: AnthropicErrorResponseDetail,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorResponseDetail {
    message: String,
}

impl From<&ChatRequest> for AnthropicRequest {
    fn from(req: &ChatRequest) -> Self {
        let mut system = None;
        let mut messages = Vec::new();

        for msg in &req.messages {
            let role = serde_json::to_value(&msg.role)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();

            if role == "system" {
                system = Some(msg.content.clone());
            } else {
                messages.push(AnthropicMessage {
                    role,
                    content: msg.content.clone(),
                });
            }
        }

        Self {
            model: req.model.clone(),
            messages,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            stream: true,
            system,
            temperature: req.temperature,
        }
    }
}

/// Parse a single SSE data line from the Anthropic stream.
fn parse_anthropic_sse(line: &str) -> Option<StreamChunk> {
    let data = line.strip_prefix("data: ")?;

    let event: AnthropicStreamEvent = match serde_json::from_str(data) {
        Ok(e) => e,
        Err(e) => return Some(StreamChunk::Error(format!("Failed to parse chunk: {e}"))),
    };

    match event.event_type.as_str() {
        "content_block_delta" => {
            if let Some(delta) = &event.delta {
                if delta.delta_type.as_deref() == Some("text_delta") {
                    if let Some(text) = &delta.text {
                        if !text.is_empty() {
                            return Some(StreamChunk::Delta(text.clone()));
                        }
                    }
                }
            }
            None
        }
        "message_stop" => Some(StreamChunk::Done),
        "message_delta" => {
            // message_delta with stop_reason indicates completion
            None
        }
        "error" => {
            let msg = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            Some(StreamChunk::Error(msg))
        }
        _ => None, // message_start, content_block_start, content_block_stop, ping
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
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
        let body = AnthropicRequest::from(&request);
        let url = self.messages_url();

        tracing::debug!("Sending Anthropic chat request to {url}");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            let message = serde_json::from_str::<AnthropicErrorResponse>(&body_text)
                .map(|e| e.error.message)
                .unwrap_or(body_text);

            return Err(LlmError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result.map_err(|e| LlmError::NetworkError(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);
            buffer.push_str(&text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() || line.starts_with("event:") {
                    continue;
                }

                if let Some(chunk) = parse_anthropic_sse(&line) {
                    let is_done = chunk == StreamChunk::Done;
                    if tx.send(chunk).is_err() {
                        return Ok(());
                    }
                    if is_done {
                        return Ok(());
                    }
                }
            }
        }

        tx.send(StreamChunk::Done).ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    #[test]
    fn anthropic_provider_name() {
        let provider = AnthropicProvider::new(
            "anthropic",
            "test-key",
            ANTHROPIC_API_URL,
            vec![ModelInfo::new("claude-sonnet-4-20250514")],
        );
        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn anthropic_provider_models() {
        let provider = AnthropicProvider::new(
            "anthropic",
            "test-key",
            ANTHROPIC_API_URL,
            vec![
                ModelInfo::new("claude-sonnet-4-20250514"),
                ModelInfo::new("claude-haiku-4-5-20251001"),
            ],
        );
        let models = provider.available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-sonnet-4-20250514");
    }

    #[test]
    fn messages_url_construction() {
        let provider = AnthropicProvider::new(
            "anthropic",
            "key",
            "https://api.anthropic.com/v1/messages",
            vec![],
        );
        assert_eq!(
            provider.messages_url(),
            "https://api.anthropic.com/v1/messages"
        );

        let provider = AnthropicProvider::new(
            "anthropic",
            "key",
            "https://api.anthropic.com/v1",
            vec![],
        );
        assert_eq!(
            provider.messages_url(),
            "https://api.anthropic.com/v1/messages"
        );

        let provider = AnthropicProvider::new(
            "anthropic",
            "key",
            "https://api.anthropic.com",
            vec![],
        );
        assert_eq!(
            provider.messages_url(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn from_env_missing_key_returns_auth_error() {
        let result = AnthropicProvider::from_env(
            "test",
            "LAZYLLM_TEST_NONEXISTENT_ANTHROPIC_KEY",
            vec![],
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            LlmError::AuthError(msg) => {
                assert!(msg.contains("LAZYLLM_TEST_NONEXISTENT_ANTHROPIC_KEY"));
            }
            other => panic!("Expected AuthError, got: {:?}", other),
        }
    }

    #[test]
    fn anthropic_request_extracts_system_message() {
        let chat_req = ChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![
                Message::system("be helpful"),
                Message::user("hello"),
            ],
        );

        let req = AnthropicRequest::from(&chat_req);
        assert_eq!(req.system.as_deref(), Some("be helpful"));
        assert_eq!(req.messages.len(), 1); // only user, system extracted
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn anthropic_request_no_system_message() {
        let chat_req = ChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![Message::user("hello")],
        );

        let req = AnthropicRequest::from(&chat_req);
        assert!(req.system.is_none());
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn anthropic_request_respects_max_tokens() {
        let chat_req = ChatRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_max_tokens(1000);

        let req = AnthropicRequest::from(&chat_req);
        assert_eq!(req.max_tokens, 1000);
    }

    #[test]
    fn anthropic_request_serializes_correctly() {
        let chat_req = ChatRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);
        let req = AnthropicRequest::from(&chat_req);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["stream"], true);
        assert_eq!(json["max_tokens"], DEFAULT_MAX_TOKENS);
        assert!(json.get("system").is_none()); // skipped when None
    }

    #[test]
    fn parse_content_block_delta() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    #[test]
    fn parse_message_stop() {
        let line = r#"data: {"type":"message_stop"}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn parse_error_event() {
        let line = r#"data: {"type":"error","error":{"message":"rate limited"}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Error("rate limited".to_string()));
    }

    #[test]
    fn parse_message_start_returns_none() {
        let line = r#"data: {"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","model":"claude-sonnet-4-20250514"}}"#;
        assert!(parse_anthropic_sse(line).is_none());
    }

    #[test]
    fn parse_content_block_start_returns_none() {
        let line = r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        assert!(parse_anthropic_sse(line).is_none());
    }

    #[test]
    fn parse_non_data_line_returns_none() {
        assert!(parse_anthropic_sse("event: content_block_delta").is_none());
        assert!(parse_anthropic_sse("").is_none());
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let line = "data: {invalid json}";
        let chunk = parse_anthropic_sse(line).unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }
}
