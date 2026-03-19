/// Compaction strategies for managing long conversations.
///
/// When a conversation approaches the context window limit, compaction
/// summarizes or truncates older messages to free up space.

use crate::llm::types::Message;

/// Available compaction strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// No compaction — just truncate to fit.
    None,
    /// Truncate oldest messages beyond the recent window.
    Truncation,
    /// Summarize older messages into a compact summary using the LLM itself.
    ClientSummarization,
}

impl Default for CompactionStrategy {
    fn default() -> Self {
        Self::Truncation
    }
}

impl CompactionStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "none" => Self::None,
            "truncation" | "truncate" => Self::Truncation,
            "summarization" | "summarize" | "client" => Self::ClientSummarization,
            _ => Self::default(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Truncation => "truncation",
            Self::ClientSummarization => "summarization",
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug)]
pub struct CompactionResult {
    /// The compacted messages to keep in the conversation.
    pub messages: Vec<Message>,
    /// A summary of what was compacted (for context assembly).
    pub summary: Option<String>,
    /// Number of messages that were removed.
    pub removed_count: usize,
}

/// Apply truncation strategy: keep only the most recent N messages.
pub fn truncate_messages(
    messages: &[Message],
    keep_recent: usize,
) -> CompactionResult {
    if messages.len() <= keep_recent {
        return CompactionResult {
            messages: messages.to_vec(),
            summary: None,
            removed_count: 0,
        };
    }

    let removed_count = messages.len() - keep_recent;
    let kept = messages[removed_count..].to_vec();

    CompactionResult {
        messages: kept,
        summary: Some(format!(
            "({removed_count} older messages were removed to fit context window)"
        )),
        removed_count,
    }
}

/// Build a summarization prompt from older messages.
/// The caller is responsible for sending this to the LLM and getting the summary back.
pub fn build_summarization_prompt(messages_to_summarize: &[Message]) -> String {
    let mut transcript = String::new();
    for msg in messages_to_summarize {
        let role = msg.role.as_str();
        transcript.push_str(&format!("{role}: {}\n", msg.content));
    }

    format!(
        "Summarize the following conversation excerpt in 2-3 concise sentences. \
         Focus on key topics, decisions, and any code/technical details discussed. \
         Do not add commentary — just summarize.\n\n{transcript}"
    )
}

/// Apply client summarization: split messages into summarized and recent portions.
/// Returns a CompactionResult with a summarization prompt as the summary field.
/// The caller should send the prompt to the LLM to get the actual summary.
pub fn prepare_client_summarization(
    messages: &[Message],
    keep_recent: usize,
) -> CompactionResult {
    if messages.len() <= keep_recent {
        return CompactionResult {
            messages: messages.to_vec(),
            summary: None,
            removed_count: 0,
        };
    }

    let split_point = messages.len() - keep_recent;
    let to_summarize = &messages[..split_point];
    let to_keep = &messages[split_point..];

    let prompt = build_summarization_prompt(to_summarize);

    CompactionResult {
        messages: to_keep.to_vec(),
        summary: Some(prompt),
        removed_count: split_point,
    }
}

/// Select the best compaction strategy based on the provider.
pub fn auto_select_strategy(provider: &str) -> CompactionStrategy {
    match provider.to_lowercase().as_str() {
        // Anthropic has generous context windows; use summarization for best quality
        "anthropic" => CompactionStrategy::ClientSummarization,
        // OpenAI also benefits from summarization
        "openai" => CompactionStrategy::ClientSummarization,
        // Google has massive context windows; truncation usually sufficient
        "google" => CompactionStrategy::Truncation,
        // Ollama / local models: keep it simple
        _ => CompactionStrategy::Truncation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("Question {i}"))
                } else {
                    Message::assistant(format!("Answer {i}"))
                }
            })
            .collect()
    }

    #[test]
    fn truncate_no_op_when_under_limit() {
        let msgs = sample_messages(5);
        let result = truncate_messages(&msgs, 10);
        assert_eq!(result.messages.len(), 5);
        assert!(result.summary.is_none());
        assert_eq!(result.removed_count, 0);
    }

    #[test]
    fn truncate_removes_oldest() {
        let msgs = sample_messages(20);
        let result = truncate_messages(&msgs, 10);
        assert_eq!(result.messages.len(), 10);
        assert_eq!(result.removed_count, 10);
        assert!(result.summary.is_some());
        // Should keep the newest messages
        assert_eq!(result.messages[0].content, "Question 10");
        assert_eq!(result.messages.last().unwrap().content, "Answer 19");
    }

    #[test]
    fn prepare_summarization_splits_correctly() {
        let msgs = sample_messages(20);
        let result = prepare_client_summarization(&msgs, 10);
        assert_eq!(result.messages.len(), 10);
        assert_eq!(result.removed_count, 10);
        assert!(result.summary.is_some());
        // Summary should contain the prompt
        let summary = result.summary.unwrap();
        assert!(summary.contains("Summarize"));
        assert!(summary.contains("Question 0"));
    }

    #[test]
    fn prepare_summarization_no_op_when_under_limit() {
        let msgs = sample_messages(5);
        let result = prepare_client_summarization(&msgs, 10);
        assert_eq!(result.messages.len(), 5);
        assert!(result.summary.is_none());
    }

    #[test]
    fn build_summarization_prompt_format() {
        let msgs = vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
        ];
        let prompt = build_summarization_prompt(&msgs);
        assert!(prompt.contains("user: What is Rust?"));
        assert!(prompt.contains("assistant: Rust is a systems programming language."));
        assert!(prompt.contains("Summarize"));
    }

    #[test]
    fn auto_select_for_anthropic() {
        assert_eq!(
            auto_select_strategy("anthropic"),
            CompactionStrategy::ClientSummarization
        );
    }

    #[test]
    fn auto_select_for_ollama() {
        assert_eq!(
            auto_select_strategy("ollama"),
            CompactionStrategy::Truncation
        );
    }

    #[test]
    fn strategy_from_str() {
        assert_eq!(CompactionStrategy::from_str("none"), CompactionStrategy::None);
        assert_eq!(CompactionStrategy::from_str("truncation"), CompactionStrategy::Truncation);
        assert_eq!(CompactionStrategy::from_str("summarize"), CompactionStrategy::ClientSummarization);
        assert_eq!(CompactionStrategy::from_str("unknown"), CompactionStrategy::Truncation);
    }

    #[test]
    fn strategy_roundtrip() {
        for s in &[CompactionStrategy::None, CompactionStrategy::Truncation, CompactionStrategy::ClientSummarization] {
            assert_eq!(CompactionStrategy::from_str(s.as_str()), *s);
        }
    }
}
