use serde::{Deserialize, Serialize};

/// Role of a message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
}

/// A request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    // TODO Phase 6: tools field for MCP integration
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
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
}

/// Incremental chunks received during streaming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamChunk {
    /// A text delta to append to the current response.
    Delta(String),
    /// The stream has completed successfully.
    Done,
    /// An error occurred during streaming.
    Error(String),
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
