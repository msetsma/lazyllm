use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::types::Message;

/// A full conversation with all messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conversation {
    pub id: Uuid,
    pub title: String,
    pub messages: Vec<Message>,
    pub model: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_name: Option<String>,
    #[serde(default)]
    pub total_input_tokens: u32,
    #[serde(default)]
    pub total_output_tokens: u32,
    #[serde(default)]
    pub total_cache_tokens: u32,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub turn_count: u32,
    #[serde(default)]
    pub context_estimate: u32,
}

impl Conversation {
    pub fn new(provider: String, model: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: "New Chat".to_string(),
            messages: Vec::new(),
            model,
            provider,
            created_at: now,
            updated_at: now,
            context_name: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_tokens: 0,
            total_cost: 0.0,
            turn_count: 0,
            context_estimate: 0,
        }
    }

    pub fn with_title(self, title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..self
        }
    }

    pub fn add_message(&self, message: Message) -> Self {
        let mut messages = self.messages.clone();
        messages.push(message);
        Self {
            messages,
            updated_at: Utc::now(),
            ..self.clone()
        }
    }

    pub fn summary(&self) -> ConversationSummary {
        ConversationSummary {
            id: self.id,
            title: self.title.clone(),
            model: self.model.clone(),
            provider: self.provider.clone(),
            message_count: self.messages.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            total_cost: self.total_cost,
            turn_count: self.turn_count,
        }
    }

    /// Generate a title from the first user message (truncated).
    pub fn auto_title(&self) -> String {
        self.messages
            .iter()
            .find(|m| m.role == crate::llm::types::Role::User)
            .map(|m| {
                let content = m.content.trim();
                if content.len() > 40 {
                    format!("{}...", &content[..37])
                } else {
                    content.to_string()
                }
            })
            .unwrap_or_else(|| "New Chat".to_string())
    }
}

/// Lightweight summary for listing conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationSummary {
    pub id: Uuid,
    pub title: String,
    pub model: String,
    pub provider: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub total_input_tokens: u32,
    #[serde(default)]
    pub total_output_tokens: u32,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub turn_count: u32,
}

/// A pre-compaction snapshot of a conversation's message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: i64,
    pub conversation_id: Uuid,
    pub snapshot_json: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Per-message usage data stored alongside a message in the DB.
#[derive(Debug, Clone, Default)]
pub struct MessageUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cost: Option<f64>,
    pub duration_ms: Option<u64>,
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    /// Verifies new conversations start with default title, empty messages, and valid timestamps.
    #[test]
    fn new_conversation_has_defaults() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        assert_eq!(conv.title, "New Chat");
        assert_eq!(conv.provider, "openai");
        assert_eq!(conv.model, "gpt-4o");
        assert!(conv.messages.is_empty());
        assert!(conv.created_at <= Utc::now());
    }

    /// Ensures with_title() returns a new conversation with the updated title.
    #[test]
    fn with_title_returns_new_conversation() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("My Chat");
        assert_eq!(conv.title, "My Chat");
    }

    /// Verifies add_message() is immutable — returns a new conversation, original unchanged.
    #[test]
    fn add_message_returns_new_conversation() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        let updated = conv.add_message(Message::user("hello"));
        assert_eq!(updated.messages.len(), 1);
        assert_eq!(updated.messages[0].content, "hello");
        assert!(conv.messages.is_empty());
    }

    /// Ensures add_message() advances the updated_at timestamp.
    #[test]
    fn add_message_updates_timestamp() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        let before = conv.updated_at;
        let updated = conv.add_message(Message::user("hi"));
        assert!(updated.updated_at >= before);
    }

    /// Verifies summary() extracts correct fields including message count.
    #[test]
    fn summary_has_correct_fields() {
        let conv = Conversation::new("anthropic".to_string(), "claude".to_string())
            .with_title("Test Chat")
            .add_message(Message::user("hi"))
            .add_message(Message::assistant("hello"));

        let summary = conv.summary();
        assert_eq!(summary.title, "Test Chat");
        assert_eq!(summary.provider, "anthropic");
        assert_eq!(summary.model, "claude");
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.id, conv.id);
    }

    /// Verifies auto_title() uses the first user message content.
    #[test]
    fn auto_title_from_first_user_message() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .add_message(Message::user("How do I write Rust?"));
        assert_eq!(conv.auto_title(), "How do I write Rust?");
    }

    /// Ensures auto_title() truncates long messages to 40 chars with ellipsis.
    #[test]
    fn auto_title_truncates_long_messages() {
        let long_msg = "a".repeat(60);
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .add_message(Message::user(&long_msg));
        let title = conv.auto_title();
        assert!(title.len() <= 40);
        assert!(title.ends_with("..."));
    }

    /// Ensures auto_title() falls back to "New Chat" when no user messages exist.
    #[test]
    fn auto_title_default_when_no_user_message() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        assert_eq!(conv.auto_title(), "New Chat");
    }

    /// Verifies auto_title() skips assistant messages and finds the first user message.
    #[test]
    fn auto_title_skips_assistant_messages() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .add_message(Message::assistant("I'm an AI"))
            .add_message(Message::user("Hello there"));
        assert_eq!(conv.auto_title(), "Hello there");
    }

    /// Ensures conversations survive JSON serialization/deserialization roundtrip.
    #[test]
    fn conversation_serializes_roundtrip() {
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Test")
            .add_message(Message::user("hi"))
            .add_message(Message::assistant("hello"));

        let json = serde_json::to_string_pretty(&conv).unwrap();
        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, conv.title);
        assert_eq!(parsed.messages.len(), conv.messages.len());
        assert_eq!(parsed.id, conv.id);
    }
}
