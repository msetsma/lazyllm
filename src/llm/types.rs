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
    /// Whether to use server-side compaction (Anthropic only).
    pub server_compaction: bool,
    /// Custom instructions to include in the compaction config (e.g. pinned message content).
    pub compaction_instructions: Option<String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            tools: None,
            server_compaction: false,
            compaction_instructions: None,
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenUsage {
    /// Tokens consumed by the prompt/input.
    pub input_tokens: u32,
    /// Tokens generated in the response/output.
    pub output_tokens: u32,
    /// Tokens read from cache (Anthropic prompt caching).
    pub cache_read_tokens: u32,
    /// Tokens written to cache (Anthropic prompt caching).
    pub cache_creation_tokens: u32,
    /// Estimated cost in USD for this turn.
    pub cost: f64,
    /// Response latency in milliseconds.
    pub duration_ms: Option<u64>,
    /// The model that produced this response.
    pub model: Option<String>,
    /// The provider that produced this response.
    pub provider: Option<String>,
}

impl TokenUsage {
    pub fn new(input_tokens: u32, output_tokens: u32) -> Self {
        Self {
            input_tokens,
            output_tokens,
            ..Default::default()
        }
    }

    /// Total tokens (input + output).
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }

    /// Total cache tokens (read + creation).
    pub fn cache_total(&self) -> u32 {
        self.cache_read_tokens + self.cache_creation_tokens
    }

    /// Accumulate another usage into this one (for partial usage like Anthropic split events).
    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cost += other.cost;
        if other.duration_ms.is_some() {
            self.duration_ms = other.duration_ms;
        }
        if other.model.is_some() {
            self.model.clone_from(&other.model);
        }
        if other.provider.is_some() {
            self.provider.clone_from(&other.provider);
        }
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
        )?;
        if self.cache_total() > 0 {
            write!(f, " (cache: {})", self.cache_total())?;
        }
        if self.cost > 0.0 {
            write!(f, " ${:.4}", self.cost)?;
        }
        Ok(())
    }
}

/// Incremental chunks received during streaming.
#[derive(Debug, Clone, PartialEq)]
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
    /// Server-side compaction occurred (e.g. Anthropic context_management).
    CompactionOccurred {
        summary_preview: String,
        messages_before: usize,
    },
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

    /// Verifies Message::user() sets Role::User and the content field.
    #[test]
    fn message_user_constructor() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    /// Verifies Message::assistant() sets Role::Assistant and the content field.
    #[test]
    fn message_assistant_constructor() {
        let msg = Message::assistant("hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "hi there");
    }

    /// Verifies Message::system() sets Role::System and the content field.
    #[test]
    fn message_system_constructor() {
        let msg = Message::system("you are helpful");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "you are helpful");
    }

    /// Verifies the builder pattern chains temperature and max_tokens onto ChatRequest.
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

    /// Ensures with_tools(vec![]) normalizes to None to avoid sending empty tool arrays.
    #[test]
    fn chat_request_with_empty_tools() {
        let req = ChatRequest::new("gpt-4o", vec![]).with_tools(vec![]);
        assert!(req.tools.is_none());
    }

    /// Verifies Message::tool_use() creates an assistant message with tool_calls attached.
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

    /// Verifies Message::tool_result() creates a tool-role message with the call ID.
    #[test]
    fn message_tool_result_constructor() {
        let msg = Message::tool_result("tc_1", "file contents here");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.content, "file contents here");
        assert_eq!(msg.tool_call_id.as_deref(), Some("tc_1"));
    }

    /// Ensures messages from older JSON format (without tool fields) deserialize correctly.
    #[test]
    fn message_backward_compat_deserialization() {
        let json = r#"{"role":"user","content":"hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
    }

    /// Ensures None tool fields are omitted from serialized JSON (skip_serializing_if).
    #[test]
    fn message_tool_calls_skip_serializing_when_none() {
        let msg = Message::user("hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("tool_calls").is_none());
        assert!(json.get("tool_call_id").is_none());
    }

    /// Verifies ModelInfo::new() defaults the display name to the model ID.
    #[test]
    fn model_info_constructor() {
        let info = ModelInfo::new("gpt-4o");
        assert_eq!(info.id, "gpt-4o");
        assert_eq!(info.name, "gpt-4o");
    }

    /// Verifies with_name() allows overriding the display name while keeping the ID.
    #[test]
    fn model_info_with_name() {
        let info = ModelInfo::new("gpt-4o").with_name("GPT-4o");
        assert_eq!(info.id, "gpt-4o");
        assert_eq!(info.name, "GPT-4o");
    }

    /// Verifies Display output includes input/output counts and total.
    #[test]
    fn token_usage_display() {
        let usage = TokenUsage::new(150, 423);
        let display = format!("{usage}");
        assert!(display.contains("150in"));
        assert!(display.contains("423out"));
        assert!(display.contains("573 tokens"));
        assert!(!display.contains("cache"));
        assert!(!display.contains("$"));
    }

    /// Verifies Display includes cache and cost when present.
    #[test]
    fn token_usage_display_with_cache_and_cost() {
        let mut usage = TokenUsage::new(100, 200);
        usage.cache_read_tokens = 50;
        usage.cost = 0.0035;
        let display = format!("{usage}");
        assert!(display.contains("cache: 50"));
        assert!(display.contains("$0.0035"));
    }

    /// Verifies accumulate() sums token counts, cost, and adopts model/provider from partials.
    #[test]
    fn token_usage_accumulate() {
        let mut total = TokenUsage::new(10, 20);
        let partial = TokenUsage {
            input_tokens: 0,
            output_tokens: 30,
            cache_read_tokens: 5,
            cost: 0.001,
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };
        total.accumulate(&partial);
        assert_eq!(total.input_tokens, 10);
        assert_eq!(total.output_tokens, 50);
        assert_eq!(total.cache_read_tokens, 5);
        assert!((total.cost - 0.001).abs() < f64::EPSILON);
        assert_eq!(total.model.as_deref(), Some("gpt-4o"));
    }

    /// Ensures duration_ms is overwritten by each accumulate (latest value wins).
    #[test]
    fn token_usage_accumulate_preserves_duration() {
        let mut total = TokenUsage::default();
        let partial = TokenUsage {
            duration_ms: Some(150),
            ..Default::default()
        };
        total.accumulate(&partial);
        assert_eq!(total.duration_ms, Some(150));

        let partial2 = TokenUsage {
            duration_ms: Some(200),
            ..Default::default()
        };
        total.accumulate(&partial2);
        assert_eq!(total.duration_ms, Some(200));
    }

    /// Ensures accumulate() does not overwrite existing model/provider with None.
    #[test]
    fn token_usage_accumulate_no_overwrite_when_none() {
        let mut total = TokenUsage {
            model: Some("gpt-4o".to_string()),
            provider: Some("openai".to_string()),
            ..Default::default()
        };
        let partial = TokenUsage {
            input_tokens: 10,
            ..Default::default()
        };
        total.accumulate(&partial);
        assert_eq!(total.model.as_deref(), Some("gpt-4o"));
        assert_eq!(total.provider.as_deref(), Some("openai"));
    }

    /// Verifies cache_total() sums read and creation cache tokens.
    #[test]
    fn token_usage_cache_total() {
        let mut usage = TokenUsage::default();
        usage.cache_read_tokens = 100;
        usage.cache_creation_tokens = 50;
        assert_eq!(usage.cache_total(), 150);
    }

    /// Ensures LlmError variants produce human-readable Display output.
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

    /// Verifies Role serializes to lowercase strings per API conventions.
    #[test]
    fn role_serializes_lowercase() {
        let json = serde_json::to_string(&Role::User).unwrap();
        assert_eq!(json, r#""user""#);
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, r#""assistant""#);
    }
}
