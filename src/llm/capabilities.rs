/// Model capabilities registry — context window sizes, max output, and feature flags.

/// Capabilities for a specific model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCapabilities {
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens per response.
    pub max_output: u32,
    /// Whether the provider supports server-side summarization (e.g. Anthropic).
    pub supports_server_compaction: bool,
    /// Whether the model supports prompt caching.
    pub supports_caching: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            max_output: 4_096,
            supports_server_compaction: false,
            supports_caching: false,
        }
    }
}

/// Look up capabilities for a model by its ID.
/// Returns sensible defaults for unknown models.
pub fn get_capabilities(model: &str) -> ModelCapabilities {
    let model_lower = model.to_lowercase();

    // OpenAI models
    if model_lower.starts_with("gpt-4o") {
        return ModelCapabilities {
            context_window: 128_000,
            max_output: 16_384,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("gpt-4-turbo") {
        return ModelCapabilities {
            context_window: 128_000,
            max_output: 4_096,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("gpt-4") {
        return ModelCapabilities {
            context_window: 8_192,
            max_output: 4_096,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("gpt-3.5") {
        return ModelCapabilities {
            context_window: 16_385,
            max_output: 4_096,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("o1") {
        return ModelCapabilities {
            context_window: 200_000,
            max_output: 100_000,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }

    // Anthropic models
    if model_lower.contains("claude-opus") || model_lower.contains("claude-4-opus") {
        return ModelCapabilities {
            context_window: 200_000,
            max_output: 32_000,
            supports_server_compaction: true,
            supports_caching: true,
        };
    }
    if model_lower.contains("claude-sonnet") || model_lower.contains("claude-4-sonnet") {
        return ModelCapabilities {
            context_window: 200_000,
            max_output: 64_000,
            supports_server_compaction: true,
            supports_caching: true,
        };
    }
    if model_lower.contains("claude-haiku") || model_lower.contains("claude-3-haiku") {
        return ModelCapabilities {
            context_window: 200_000,
            max_output: 8_192,
            supports_server_compaction: true,
            supports_caching: true,
        };
    }

    // Google Gemini models
    if model_lower.starts_with("gemini-2.5") || model_lower.starts_with("gemini-2.0") {
        return ModelCapabilities {
            context_window: 1_048_576,
            max_output: 65_536,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("gemini-1.5-pro") {
        return ModelCapabilities {
            context_window: 2_097_152,
            max_output: 8_192,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }
    if model_lower.starts_with("gemini-1.5-flash") {
        return ModelCapabilities {
            context_window: 1_048_576,
            max_output: 8_192,
            supports_server_compaction: false,
            supports_caching: false,
        };
    }

    // Ollama / unknown — conservative defaults
    ModelCapabilities::default()
}

/// Estimate token count from text using the char/4 heuristic.
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as f64 / 4.0).ceil() as u32
}

/// Estimate token count for a set of messages.
pub fn estimate_message_tokens(messages: &[crate::llm::types::Message]) -> u32 {
    messages
        .iter()
        .map(|m| {
            // ~4 tokens overhead per message for role/formatting
            let overhead = 4u32;
            let content_tokens = estimate_tokens(&m.content);
            let tool_tokens = m
                .tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .map(|c| estimate_tokens(&c.name) + estimate_tokens(&c.arguments))
                        .sum::<u32>()
                })
                .unwrap_or(0);
            overhead + content_tokens + tool_tokens
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;

    #[test]
    fn gpt4o_capabilities() {
        let caps = get_capabilities("gpt-4o");
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.max_output, 16_384);
        assert!(!caps.supports_server_compaction);
    }

    #[test]
    fn claude_sonnet_capabilities() {
        let caps = get_capabilities("claude-sonnet-4-20250514");
        assert_eq!(caps.context_window, 200_000);
        assert!(caps.supports_server_compaction);
        assert!(caps.supports_caching);
    }

    #[test]
    fn gemini_capabilities() {
        let caps = get_capabilities("gemini-2.5-pro");
        assert_eq!(caps.context_window, 1_048_576);
    }

    #[test]
    fn unknown_model_gets_defaults() {
        let caps = get_capabilities("llama3.2:8b");
        assert_eq!(caps.context_window, 128_000);
        assert!(!caps.supports_server_compaction);
    }

    #[test]
    fn estimate_tokens_basic() {
        // "hello" = 5 chars => ceil(5/4) = 2
        assert_eq!(estimate_tokens("hello"), 2);
        // Empty
        assert_eq!(estimate_tokens(""), 0);
        // 400 chars => 100 tokens
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn estimate_message_tokens_basic() {
        let msgs = vec![
            Message::user("hello"),       // 4 overhead + 2 content = 6
            Message::assistant("hi back"), // 4 overhead + 2 content = 6
        ];
        let tokens = estimate_message_tokens(&msgs);
        assert!(tokens > 0);
        // ~12 tokens for these two messages
        assert!(tokens >= 10 && tokens <= 16);
    }

    #[test]
    fn context_usage_percentage() {
        let caps = get_capabilities("gpt-4o");
        let msgs = vec![Message::user("hello")];
        let used = estimate_message_tokens(&msgs);
        let pct = (used as f64 / caps.context_window as f64) * 100.0;
        assert!(pct < 1.0); // tiny message, should be near 0%
    }

    #[test]
    fn o1_capabilities() {
        let caps = get_capabilities("o1-preview");
        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.max_output, 100_000);
    }

    #[test]
    fn gpt35_capabilities() {
        let caps = get_capabilities("gpt-3.5-turbo");
        assert_eq!(caps.context_window, 16_385);
    }

    #[test]
    fn gpt4_turbo_capabilities() {
        let caps = get_capabilities("gpt-4-turbo");
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.max_output, 4_096);
    }

    #[test]
    fn claude_opus_capabilities() {
        let caps = get_capabilities("claude-opus-4-20250514");
        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.max_output, 32_000);
        assert!(caps.supports_server_compaction);
        assert!(caps.supports_caching);
    }

    #[test]
    fn claude_haiku_capabilities() {
        let caps = get_capabilities("claude-3-haiku-20240307");
        assert_eq!(caps.context_window, 200_000);
        assert!(caps.supports_caching);
    }

    #[test]
    fn gemini_flash_capabilities() {
        let caps = get_capabilities("gemini-1.5-flash");
        assert_eq!(caps.context_window, 1_048_576);
    }

    #[test]
    fn estimate_message_tokens_with_tool_calls() {
        use crate::llm::types::ToolCall;
        let msg = Message::tool_use(vec![ToolCall {
            id: "tc_1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp/test.txt"}"#.to_string(),
        }]);
        let tokens = estimate_message_tokens(&[msg]);
        // Should include overhead + tool name + arguments
        assert!(tokens > 4);
    }

    #[test]
    fn estimate_message_tokens_empty_list() {
        assert_eq!(estimate_message_tokens(&[]), 0);
    }

    #[test]
    fn default_capabilities() {
        let caps = ModelCapabilities::default();
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.max_output, 4_096);
        assert!(!caps.supports_server_compaction);
        assert!(!caps.supports_caching);
    }
}
