use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::streaming::{check_http_error, stream_sse_response};
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk, ToolCall, ToolDefinition, TokenUsage};

#[allow(dead_code)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicMessageContent,
}

/// SSE event types from the Anthropic API.
#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    #[allow(dead_code)]
    index: Option<usize>,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    error: Option<AnthropicErrorDetail>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlockStart>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlockStart {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    #[serde(default)]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
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

fn convert_anthropic_tools(tools: &Option<Vec<ToolDefinition>>) -> Option<Vec<AnthropicTool>> {
    tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| AnthropicTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect()
    })
}

impl From<&ChatRequest> for AnthropicRequest {
    fn from(req: &ChatRequest) -> Self {
        let mut system = None;
        let mut messages = Vec::new();

        for msg in &req.messages {
            let role = msg.role.as_str();

            if role == "system" {
                system = Some(msg.content.clone());
                continue;
            }

            // Handle assistant messages with tool calls
            if let Some(tool_calls) = &msg.tool_calls {
                let mut blocks = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: msg.content.clone(),
                    });
                }
                for tc in tool_calls {
                    let input: serde_json::Value =
                        serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                    blocks.push(AnthropicContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input,
                    });
                }
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: AnthropicMessageContent::Blocks(blocks),
                });
                continue;
            }

            // Handle tool result messages
            if let Some(tool_call_id) = &msg.tool_call_id {
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(vec![
                        AnthropicContentBlock::ToolResult {
                            tool_use_id: tool_call_id.clone(),
                            content: msg.content.clone(),
                        },
                    ]),
                });
                continue;
            }

            // Regular messages
            messages.push(AnthropicMessage {
                role: role.to_string(),
                content: AnthropicMessageContent::Text(msg.content.clone()),
            });
        }

        Self {
            model: req.model.clone(),
            messages,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            stream: true,
            system,
            temperature: req.temperature,
            tools: convert_anthropic_tools(&req.tools),
        }
    }
}

/// Accumulated state for Anthropic tool use blocks during streaming.
#[derive(Debug, Default)]
struct AnthropicToolAccumulator {
    /// Currently accumulating tool call, if any.
    current: Option<AccumulatingToolBlock>,
}

#[derive(Debug, Default)]
struct AccumulatingToolBlock {
    id: String,
    name: String,
    arguments_json: String,
}

impl AnthropicToolAccumulator {
    fn start_tool(&mut self, id: String, name: String) {
        self.current = Some(AccumulatingToolBlock {
            id,
            name,
            arguments_json: String::new(),
        });
    }

    fn append_json(&mut self, json: &str) {
        if let Some(current) = &mut self.current {
            current.arguments_json.push_str(json);
        }
    }

    fn finish_tool(&mut self) -> Option<ToolCall> {
        self.current.take().map(|block| ToolCall {
            id: block.id,
            name: block.name,
            arguments: block.arguments_json,
        })
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
            if let Some(delta) = &event.delta
                && delta.delta_type.as_deref() == Some("text_delta")
                && let Some(text) = &delta.text
                && !text.is_empty()
            {
                return Some(StreamChunk::Delta(text.clone()));
            }
            None
        }
        "message_start" => {
            if let Some(msg) = &event.message
                && let Some(usage) = &msg.usage
            {
                let mut tu = TokenUsage::new(usage.input_tokens.unwrap_or(0), 0);
                tu.cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0);
                tu.cache_creation_tokens = usage.cache_creation_input_tokens.unwrap_or(0);
                return Some(StreamChunk::Usage(tu));
            }
            None
        }
        "message_delta" => {
            if let Some(usage) = &event.usage
                && let Some(output) = usage.output_tokens
            {
                return Some(StreamChunk::Usage(TokenUsage::new(0, output)));
            }
            None
        }
        "message_stop" => Some(StreamChunk::Done),
        "error" => {
            let msg = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            Some(StreamChunk::Error(msg))
        }
        _ => None,
    }
}

/// Parse an Anthropic SSE line with tool use awareness.
fn parse_anthropic_sse_with_tools(
    line: &str,
    accumulator: &mut AnthropicToolAccumulator,
) -> Option<StreamChunk> {
    let data = line.strip_prefix("data: ")?;

    let event: AnthropicStreamEvent = match serde_json::from_str(data) {
        Ok(e) => e,
        Err(e) => return Some(StreamChunk::Error(format!("Failed to parse chunk: {e}"))),
    };

    match event.event_type.as_str() {
        "content_block_start" => {
            if let Some(block) = &event.content_block
                && block.block_type == "tool_use"
            {
                let id = block.id.clone().unwrap_or_default();
                let name = block.name.clone().unwrap_or_default();
                accumulator.start_tool(id, name);
            }
            None
        }
        "content_block_delta" => {
            if let Some(delta) = &event.delta {
                if delta.delta_type.as_deref() == Some("text_delta") {
                    if let Some(text) = &delta.text
                        && !text.is_empty()
                    {
                        return Some(StreamChunk::Delta(text.clone()));
                    }
                }
                if delta.delta_type.as_deref() == Some("input_json_delta") {
                    if let Some(json) = &delta.partial_json {
                        accumulator.append_json(json);
                    }
                }
            }
            None
        }
        "content_block_stop" => {
            // If we were accumulating a tool call, emit it
            if let Some(tc) = accumulator.finish_tool() {
                return Some(StreamChunk::ToolCallStart {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments,
                });
            }
            None
        }
        "message_start" => {
            if let Some(msg) = &event.message
                && let Some(usage) = &msg.usage
            {
                let mut tu = TokenUsage::new(usage.input_tokens.unwrap_or(0), 0);
                tu.cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0);
                tu.cache_creation_tokens = usage.cache_creation_input_tokens.unwrap_or(0);
                return Some(StreamChunk::Usage(tu));
            }
            None
        }
        "message_delta" => {
            if let Some(usage) = &event.usage
                && let Some(output) = usage.output_tokens
            {
                return Some(StreamChunk::Usage(TokenUsage::new(0, output)));
            }
            None
        }
        "message_stop" => Some(StreamChunk::Done),
        "error" => {
            let msg = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            Some(StreamChunk::Error(msg))
        }
        _ => None,
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
        let has_tools = request.tools.is_some();
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

        let response = check_http_error(response, |body| {
            serde_json::from_str::<AnthropicErrorResponse>(body)
                .map(|e| e.error.message)
                .ok()
        })
        .await?;

        if has_tools {
            let mut accumulator = AnthropicToolAccumulator::default();
            stream_sse_response(
                response,
                &tx,
                move |line| parse_anthropic_sse_with_tools(line, &mut accumulator),
                |line| line.starts_with("event:"),
            )
            .await
        } else {
            stream_sse_response(
                response,
                &tx,
                parse_anthropic_sse,
                |line| line.starts_with("event:"),
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    /// Verifies the provider reports the correct name for identification.
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

    /// Ensures configured models are returned in order from available_models().
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

    /// Ensures the messages URL is normalized regardless of trailing path variants.
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

    /// Verifies system messages are extracted into the top-level `system` field per Anthropic API.
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
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
        assert!(req.tools.is_none());
    }

    /// Ensures requests without system messages leave the system field as None.
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

    /// Verifies tool definitions are converted to the Anthropic tool format.
    #[test]
    fn anthropic_request_with_tools() {
        let tools = vec![super::super::types::ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let chat_req = ChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![Message::user("hi")],
        )
        .with_tools(tools);
        let req = AnthropicRequest::from(&chat_req);
        assert!(req.tools.is_some());
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
        assert_eq!(req.tools.as_ref().unwrap()[0].name, "read_file");
    }

    /// Ensures the max_tokens override from ChatRequest is forwarded correctly.
    #[test]
    fn anthropic_request_respects_max_tokens() {
        let chat_req = ChatRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_max_tokens(1000);

        let req = AnthropicRequest::from(&chat_req);
        assert_eq!(req.max_tokens, 1000);
    }

    /// Verifies JSON serialization omits None fields (system, tools) per skip_serializing_if.
    #[test]
    fn anthropic_request_serializes_correctly() {
        let chat_req = ChatRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);
        let req = AnthropicRequest::from(&chat_req);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["stream"], true);
        assert_eq!(json["max_tokens"], DEFAULT_MAX_TOKENS);
        assert!(json.get("system").is_none());
        assert!(json.get("tools").is_none());
    }

    /// Verifies content_block_delta SSE events are parsed into text Delta chunks.
    #[test]
    fn parse_content_block_delta() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    /// Verifies message_stop SSE events signal stream completion.
    #[test]
    fn parse_message_stop() {
        let line = r#"data: {"type":"message_stop"}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    /// Verifies error events are parsed into StreamChunk::Error with the message.
    #[test]
    fn parse_error_event() {
        let line = r#"data: {"type":"error","error":{"message":"rate limited"}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(chunk, StreamChunk::Error("rate limited".to_string()));
    }

    /// Ensures message_start events without usage data are ignored.
    #[test]
    fn parse_message_start_without_usage_returns_none() {
        let line = r#"data: {"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","model":"claude-sonnet-4-20250514"}}"#;
        assert!(parse_anthropic_sse(line).is_none());
    }

    /// Verifies input token counts are extracted from message_start usage data.
    #[test]
    fn parse_message_start_with_usage_returns_input_tokens() {
        let line = r#"data: {"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","usage":{"input_tokens":42}}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(
            chunk,
            StreamChunk::Usage(super::super::types::TokenUsage::new(42, 0))
        );
    }

    /// Verifies output token counts are extracted from message_delta usage data.
    #[test]
    fn parse_message_delta_with_usage_returns_output_tokens() {
        let line = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":87}}"#;
        let chunk = parse_anthropic_sse(line).unwrap();
        assert_eq!(
            chunk,
            StreamChunk::Usage(super::super::types::TokenUsage::new(0, 87))
        );
    }

    /// Ensures non-data SSE lines (event:, empty) are silently skipped.
    #[test]
    fn parse_non_data_line_returns_none() {
        assert!(parse_anthropic_sse("event: content_block_delta").is_none());
        assert!(parse_anthropic_sse("").is_none());
    }

    /// Ensures malformed JSON in SSE data produces an Error chunk rather than panicking.
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
