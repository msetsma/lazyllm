use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::LlmProvider;
use super::streaming::{check_http_error, stream_sse_response, stream_sse_response_multi};
use super::types::{ChatRequest, LlmError, ModelInfo, StreamChunk, ToolCall, ToolDefinition, TokenUsage};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Debug, Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiMessageToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessageToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAiMessageFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiMessageFunction {
    name: String,
    arguments: String,
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
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
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

fn convert_tool_definitions(tools: &Option<Vec<ToolDefinition>>) -> Option<Vec<OpenAiTool>> {
    tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| OpenAiTool {
                tool_type: "function".to_string(),
                function: OpenAiFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect()
    })
}

impl From<&ChatRequest> for OpenAiRequest {
    fn from(req: &ChatRequest) -> Self {
        let messages = req
            .messages
            .iter()
            .map(|m| {
                // Handle tool call messages (assistant requesting tools)
                if let Some(tool_calls) = &m.tool_calls {
                    return OpenAiMessage {
                        role: "assistant".to_string(),
                        content: if m.content.is_empty() {
                            None
                        } else {
                            Some(m.content.clone())
                        },
                        tool_calls: Some(
                            tool_calls
                                .iter()
                                .map(|tc| OpenAiMessageToolCall {
                                    id: tc.id.clone(),
                                    call_type: "function".to_string(),
                                    function: OpenAiMessageFunction {
                                        name: tc.name.clone(),
                                        arguments: tc.arguments.clone(),
                                    },
                                })
                                .collect(),
                        ),
                        tool_call_id: None,
                    };
                }
                // Handle tool result messages
                if let Some(tool_call_id) = &m.tool_call_id {
                    return OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(m.content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tool_call_id.clone()),
                    };
                }
                // Regular messages
                OpenAiMessage {
                    role: m.role.as_str().to_string(),
                    content: Some(m.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                }
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
            tools: convert_tool_definitions(&req.tools),
        }
    }
}

/// Accumulated state for tool call deltas during streaming.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    calls: Vec<AccumulatedToolCall>,
}

#[derive(Debug, Default, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn accumulate(&mut self, deltas: &[OpenAiToolCallDelta]) {
        for delta in deltas {
            // Grow the vec if needed
            while self.calls.len() <= delta.index {
                self.calls.push(AccumulatedToolCall::default());
            }
            let entry = &mut self.calls[delta.index];
            if let Some(id) = &delta.id {
                entry.id = id.clone();
            }
            if let Some(func) = &delta.function {
                if let Some(name) = &func.name {
                    entry.name = name.clone();
                }
                if let Some(args) = &func.arguments {
                    entry.arguments.push_str(args);
                }
            }
        }
    }

    fn into_tool_calls(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .map(|c| ToolCall {
                id: c.id,
                name: c.name,
                arguments: c.arguments,
            })
            .collect()
    }
}

/// Parse a single SSE data line into a StreamChunk.
/// Returns an additional flag indicating tool call deltas that need accumulation.
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

/// Parse an SSE line with tool call accumulation, returning multiple chunks if needed.
fn parse_sse_line_with_tools(
    line: &str,
    accumulator: &mut ToolCallAccumulator,
) -> Vec<StreamChunk> {
    let Some(data) = line.strip_prefix("data: ") else {
        return vec![];
    };

    if data.trim() == "[DONE]" {
        return vec![StreamChunk::Done];
    }

    let chunk: OpenAiStreamChunk = match serde_json::from_str(data) {
        Ok(c) => c,
        Err(e) => return vec![StreamChunk::Error(format!("Failed to parse chunk: {e}"))],
    };

    if let Some(usage) = chunk.usage {
        return vec![StreamChunk::Usage(TokenUsage::new(
            usage.prompt_tokens,
            usage.completion_tokens,
        ))];
    }

    if let Some(choice) = chunk.choices.first() {
        // Accumulate tool call deltas
        if let Some(tool_calls) = &choice.delta.tool_calls {
            accumulator.accumulate(tool_calls);
        }

        if let Some(ref content) = choice.delta.content
            && !content.is_empty()
        {
            return vec![StreamChunk::Delta(content.clone())];
        }

        if choice.finish_reason.as_deref() == Some("tool_calls") {
            let tool_calls = std::mem::take(accumulator);
            return tool_calls
                .into_tool_calls()
                .into_iter()
                .map(|tc| StreamChunk::ToolCallStart {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments,
                })
                .collect();
        }

        if choice.finish_reason.as_deref() == Some("stop") {
            return vec![StreamChunk::Done];
        }
    }

    vec![]
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
        let has_tools = request.tools.is_some();
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

        if has_tools {
            let mut accumulator = ToolCallAccumulator::default();
            stream_sse_response_multi(
                response,
                &tx,
                move |line| parse_sse_line_with_tools(line, &mut accumulator),
                |_| false,
            )
            .await
        } else {
            stream_sse_response(response, &tx, parse_sse_line, |_| false).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    /// Verifies the provider reports the correct name for identification.
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

    /// Ensures configured models are returned in order from available_models().
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

    /// Ensures the chat completions URL is normalized with or without trailing slash.
    #[test]
    fn chat_url_construction() {
        let provider = OpenAiProvider::new(
            "openai",
            "key",
            "https://api.openai.com/v1",
            vec![],
        );
        assert_eq!(provider.chat_url(), "https://api.openai.com/v1/chat/completions");

        let provider = OpenAiProvider::new(
            "openai",
            "key",
            "https://api.openai.com/v1/",
            vec![],
        );
        assert_eq!(provider.chat_url(), "https://api.openai.com/v1/chat/completions");
    }

    /// Verifies ChatRequest fields (messages, temperature, max_tokens) are mapped correctly.
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
        assert_eq!(openai_req.messages[0].content.as_deref(), Some("be helpful"));
        assert_eq!(openai_req.messages[1].role, "user");
        assert_eq!(openai_req.messages[1].content.as_deref(), Some("hello"));
        assert_eq!(openai_req.temperature, Some(0.7));
        assert_eq!(openai_req.max_tokens, Some(500));
    }

    /// Verifies JSON serialization omits optional fields (temperature, tools) when not set.
    #[test]
    fn openai_request_serializes_correctly() {
        let chat_req = ChatRequest::new("gpt-4o", vec![Message::user("hi")]);
        let openai_req = OpenAiRequest::from(&chat_req);
        let json = serde_json::to_value(&openai_req).unwrap();

        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["stream"], true);
        assert!(json.get("temperature").is_none());
        assert!(json.get("max_tokens").is_none());
        assert!(json.get("tools").is_none());
    }

    /// Verifies tool definitions are wrapped in OpenAI's {type: "function", function: ...} format.
    #[test]
    fn openai_request_with_tools() {
        let tools = vec![super::super::types::ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];
        let chat_req = ChatRequest::new("gpt-4o", vec![Message::user("hi")]).with_tools(tools);
        let openai_req = OpenAiRequest::from(&chat_req);
        let json = serde_json::to_value(&openai_req).unwrap();

        assert!(json.get("tools").is_some());
        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "read_file");
    }

    /// Verifies tool_use and tool_result messages are mapped to OpenAI's assistant/tool roles.
    #[test]
    fn openai_request_with_tool_messages() {
        let tool_calls = vec![super::super::types::ToolCall {
            id: "call_123".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp/test"}"#.to_string(),
        }];
        let messages = vec![
            Message::user("read the file"),
            Message::tool_use(tool_calls),
            Message::tool_result("call_123", "file contents"),
        ];
        let chat_req = ChatRequest::new("gpt-4o", messages);
        let openai_req = OpenAiRequest::from(&chat_req);

        assert_eq!(openai_req.messages.len(), 3);
        assert_eq!(openai_req.messages[1].role, "assistant");
        assert!(openai_req.messages[1].tool_calls.is_some());
        assert_eq!(openai_req.messages[2].role, "tool");
        assert_eq!(openai_req.messages[2].tool_call_id.as_deref(), Some("call_123"));
    }

    /// Verifies streamed tool call deltas are accumulated and reassembled correctly.
    #[test]
    fn tool_call_accumulator_basic() {
        let mut acc = ToolCallAccumulator::default();
        acc.accumulate(&[OpenAiToolCallDelta {
            index: 0,
            id: Some("call_1".to_string()),
            function: Some(OpenAiFunctionDelta {
                name: Some("read_file".to_string()),
                arguments: Some(r#"{"pa"#.to_string()),
            }),
        }]);
        acc.accumulate(&[OpenAiToolCallDelta {
            index: 0,
            id: None,
            function: Some(OpenAiFunctionDelta {
                name: None,
                arguments: Some(r#"th":"/"}"#.to_string()),
            }),
        }]);

        let calls = acc.into_tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, r#"{"path":"/"}"#);
    }

    /// Verifies SSE delta content is parsed into text Delta chunks.
    #[test]
    fn parse_sse_delta_line() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Delta("Hello".to_string()));
    }

    /// Verifies the [DONE] sentinel is parsed as stream completion.
    #[test]
    fn parse_sse_done_line() {
        let line = "data: [DONE]";
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    /// Verifies finish_reason="stop" also signals stream completion.
    #[test]
    fn parse_sse_finish_reason_stop() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    /// Ensures role-only initial deltas (no content) are skipped.
    #[test]
    fn parse_sse_empty_delta_returns_none() {
        let line = r#"data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let chunk = parse_sse_line(line);
        assert!(chunk.is_none());
    }

    /// Ensures non-data SSE lines (event:, comments, empty) are silently skipped.
    #[test]
    fn parse_sse_non_data_line_returns_none() {
        assert!(parse_sse_line("event: message").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": comment").is_none());
    }

    /// Ensures malformed JSON produces an Error chunk rather than panicking.
    #[test]
    fn parse_sse_invalid_json_returns_error() {
        let line = "data: {invalid json}";
        let chunk = parse_sse_line(line).unwrap();
        match chunk {
            StreamChunk::Error(msg) => assert!(msg.contains("Failed to parse")),
            other => panic!("Expected Error, got: {:?}", other),
        }
    }

    /// Verifies usage data (prompt_tokens, completion_tokens) is extracted from SSE chunks.
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
