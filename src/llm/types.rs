use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Role of a message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        }
    }
}

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Tool calls requested by the assistant (OpenAI/Anthropic format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message is a result for.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with tool calls (no text content).
    pub fn tool_use(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// A tool call requested by the assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A tool definition to pass in a chat request.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<ToolDefinition>>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            tools: None,
        }
    }

    pub fn with_temperature(self, temperature: f32) -> Self {
        Self {
            temperature: Some(temperature),
            ..self
        }
    }

    pub fn with_max_tokens(self, max_tokens: u32) -> Self {
        Self {
            max_tokens: Some(max_tokens),
            ..self
        }
    }

    pub fn with_tools(self, tools: Vec<ToolDefinition>) -> Self {
        Self {
            tools: if tools.is_empty() { None } else { Some(tools) },
            ..self
        }
    }
}

/// Token usage statistics returned by providers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Tokens consumed by the prompt/input.
    pub input_tokens: u32,
    /// Tokens generated in the response/output.
    pub output_tokens: u32,
}

impl TokenUsage {
    pub fn new(input_tokens: u32, output_tokens: u32) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }

    /// Total tokens (input + output).
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

impl std::fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}in + {}out = {} tokens",
            self.input_tokens,
            self.output_tokens,
            self.total()
        )
    }
}

/// Incremental chunks received during streaming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamChunk {
    /// A text delta to append to the current response.
    Delta(String),
    /// Token usage statistics for this response.
    Usage(TokenUsage),
    /// The stream has completed successfully.
    Done,
    /// An error occurred during streaming.
    Error(String),
    /// The LLM is requesting a tool call.
    ToolCallStart { id: String, name: String, arguments: String },
    /// Result from executing a tool call.
    ToolCallResult { id: String, content: String, is_error: bool },
}

/// Information about an available model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let name = id.clone();
        Self { id, name }
    }

    pub fn with_name(self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }
}

/// Errors that can occur during LLM operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// Missing API key or invalid authentication.
    AuthError(String),
    /// Network or connection error.
    NetworkError(String),
    /// The provider returned an error response.
    ApiError { status: u16, message: String },
    /// Failed to parse the response.
    ParseError(String),
    /// The provider or model is not supported.
    Unsupported(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::AuthError(msg) => write!(f, "Authentication error: {msg}"),
            LlmError::NetworkError(msg) => write!(f, "Network error: {msg}"),
            LlmError::ApiError { status, message } => {
                write!(f, "API error ({status}): {message}")
            }
            LlmError::ParseError(msg) => write!(f, "Parse error: {msg}"),
            LlmError::Unsupported(msg) => write!(f, "Unsupported: {msg}"),
        }
    }
}

impl std::error::Error for LlmError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_user_constructor() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn message_assistant_constructor() {
        let msg = Message::assistant("hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "hi there");
    }

    #[test]
    fn message_system_constructor() {
        let msg = Message::system("you are helpful");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "you are helpful");
    }

    #[test]
    fn chat_request_builder() {
        let req = ChatRequest::new("gpt-4o", vec![Message::user("hi")])
            .with_temperature(0.7)
            .with_max_tokens(1000);
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(1000));
    }

    #[test]
    fn chat_request_defaults() {
        let req = ChatRequest::new("gpt-4o", vec![]);
        assert!(req.temperature.is_none());
        assert!(req.max_tokens.is_none());
        assert!(req.tools.is_none());
    }

    #[test]
    fn chat_request_with_tools() {
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let req = ChatRequest::new("gpt-4o", vec![]).with_tools(tools);
        assert!(req.tools.is_some());
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn chat_request_with_empty_tools() {
        let req = ChatRequest::new("gpt-4o", vec![]).with_tools(vec![]);
        assert!(req.tools.is_none());
    }

    #[test]
    fn message_tool_use_constructor() {
        let tool_calls = vec![ToolCall {
            id: "tc_1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp/test"}"#.to_string(),
        }];
        let msg = Message::tool_use(tool_calls.clone());
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.content.is_empty());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn message_tool_result_constructor() {
        let msg = Message::tool_result("tc_1", "file contents here");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.content, "file contents here");
        assert_eq!(msg.tool_call_id.as_deref(), Some("tc_1"));
    }

    #[test]
    fn message_backward_compat_deserialization() {
        // Old-format JSON without tool_calls/tool_call_id should still work
        let json = r#"{"role":"user","content":"hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
    }

    #[test]
    fn message_tool_calls_skip_serializing_when_none() {
        let msg = Message::user("hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("tool_calls").is_none());
        assert!(json.get("tool_call_id").is_none());
    }

    #[test]
    fn model_info_constructor() {
        let info = ModelInfo::new("gpt-4o");
        assert_eq!(info.id, "gpt-4o");
        assert_eq!(info.name, "gpt-4o");
    }

    #[test]
    fn model_info_with_name() {
        let info = ModelInfo::new("gpt-4o").with_name("GPT-4o");
        assert_eq!(info.id, "gpt-4o");
        assert_eq!(info.name, "GPT-4o");
    }

    #[test]
    fn stream_chunk_variants() {
        let delta = StreamChunk::Delta("hello".to_string());
        assert_eq!(delta, StreamChunk::Delta("hello".to_string()));

        let done = StreamChunk::Done;
        assert_eq!(done, StreamChunk::Done);

        let err = StreamChunk::Error("oops".to_string());
        assert_eq!(err, StreamChunk::Error("oops".to_string()));

        let usage = StreamChunk::Usage(TokenUsage::new(10, 20));
        assert_eq!(usage, StreamChunk::Usage(TokenUsage::new(10, 20)));
    }

    #[test]
    fn token_usage_new_and_total() {
        let usage = TokenUsage::new(100, 200);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total(), 300);
    }

    #[test]
    fn token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn token_usage_display() {
        let usage = TokenUsage::new(150, 423);
        let display = format!("{usage}");
        assert!(display.contains("150in"));
        assert!(display.contains("423out"));
        assert!(display.contains("573 tokens"));
    }

    #[test]
    fn llm_error_display() {
        let err = LlmError::AuthError("missing key".to_string());
        assert!(err.to_string().contains("missing key"));

        let err = LlmError::ApiError {
            status: 429,
            message: "rate limited".to_string(),
        };
        assert!(err.to_string().contains("429"));
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn message_serializes_to_json() {
        let msg = Message::user("hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello");
    }

    #[test]
    fn message_deserializes_from_json() {
        let json = r#"{"role":"assistant","content":"hi"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "hi");
    }

    #[test]
    fn role_serializes_lowercase() {
        let json = serde_json::to_string(&Role::User).unwrap();
        assert_eq!(json, r#""user""#);
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, r#""assistant""#);
    }
}
