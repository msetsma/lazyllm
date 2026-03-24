/// Compaction strategies for managing long conversations.
///
/// When a conversation approaches the context window limit, compaction
/// summarizes or truncates older messages to free up space.

use tokio::sync::mpsc;

use crate::config::types::ConversationConfig;
use crate::llm::capabilities::{estimate_message_tokens, ModelCapabilities};
use crate::llm::types::{ChatRequest, LlmError, Message, StreamChunk};
use crate::llm::LlmProvider;
use crate::store::types::CompactionMode;

/// Available compaction strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// No compaction.
    None,
    /// Clear old tool results without removing messages (cheapest).
    ToolClearing,
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
            "tool_clearing" | "tools" => Self::ToolClearing,
            "truncation" | "truncate" => Self::Truncation,
            "summarization" | "summarize" | "client" => Self::ClientSummarization,
            _ => Self::default(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ToolClearing => "tool_clearing",
            Self::Truncation => "truncation",
            Self::ClientSummarization => "summarization",
        }
    }
}

// ── Compaction profiles ─────────────────────────────────────────────

/// Context window size tier for automatic profile selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTier {
    /// ≤8k tokens — very small local models.
    Tiny,
    /// 8k–32k tokens — small models.
    Small,
    /// 32k–200k tokens — most cloud models (GPT-4o, Claude, etc).
    Medium,
    /// >200k tokens — large context models (Gemini, Claude with extended).
    Large,
}

impl ContextTier {
    pub fn from_context_window(window: u32) -> Self {
        match window {
            0..=8_000 => Self::Tiny,
            8_001..=32_000 => Self::Small,
            32_001..=200_000 => Self::Medium,
            _ => Self::Large,
        }
    }
}

/// Per-model compaction profile controlling thresholds, strategy pipeline,
/// and recent message count.
#[derive(Debug, Clone)]
pub struct CompactionProfile {
    pub tier: ContextTier,
    /// Context usage fraction that triggers compaction recommendation.
    pub trigger_threshold: f64,
    /// Target usage fraction after compaction completes.
    pub target_after: f64,
    /// Number of recent messages to always preserve.
    pub recent_messages: usize,
    /// Ordered list of strategies to try (cheapest first).
    pub pipeline: Vec<CompactionStrategy>,
    /// Whether this model can produce useful summaries of its own context.
    pub can_self_summarize: bool,
}

/// User config overrides that take priority over tier defaults.
#[derive(Debug, Clone, Default)]
pub struct ProfileOverrides {
    pub compaction_threshold: Option<f64>,
    pub recent_messages: Option<usize>,
    /// If "none", disables the pipeline entirely.
    pub compaction_strategy: Option<String>,
}

impl ProfileOverrides {
    /// Build overrides from `ConversationConfig`, treating default values as "not set".
    pub fn from_config(config: &ConversationConfig) -> Self {
        // Only override when the user explicitly changed from defaults.
        let threshold = if (config.compaction_threshold - 0.75).abs() > f64::EPSILON {
            Some(config.compaction_threshold)
        } else {
            None
        };
        let recent = if config.recent_messages != 20 {
            Some(config.recent_messages)
        } else {
            None
        };
        let strategy = if config.compaction_strategy != "auto" {
            Some(config.compaction_strategy.clone())
        } else {
            None
        };
        Self {
            compaction_threshold: threshold,
            recent_messages: recent,
            compaction_strategy: strategy,
        }
    }
}

/// Derive a compaction profile from model capabilities, provider name, and user overrides.
///
/// Tier is selected by context window size. Provider name influences `can_self_summarize`
/// (Ollama defaults to false). User overrides win when explicitly set.
pub fn derive_profile(
    capabilities: &ModelCapabilities,
    provider_name: &str,
    overrides: &ProfileOverrides,
) -> CompactionProfile {
    let tier = ContextTier::from_context_window(capabilities.context_window);
    let is_ollama = provider_name.to_lowercase().contains("ollama");

    // Check if strategy is explicitly disabled
    if let Some(ref strategy) = overrides.compaction_strategy {
        if strategy == "none" {
            return CompactionProfile {
                tier,
                trigger_threshold: 1.0, // never triggers
                target_after: 0.0,
                recent_messages: overrides.recent_messages.unwrap_or(20),
                pipeline: vec![],
                can_self_summarize: false,
            };
        }
    }

    let (base_threshold, base_target, base_recent, base_pipeline, base_summarize) = match tier {
        ContextTier::Tiny => (
            0.50,
            0.20,
            4usize,
            vec![CompactionStrategy::Truncation],
            false,
        ),
        ContextTier::Small => (
            0.60,
            0.25,
            8,
            vec![CompactionStrategy::Truncation],
            false,
        ),
        ContextTier::Medium => (
            0.70,
            0.30,
            15,
            vec![
                CompactionStrategy::ClientSummarization,
            ],
            !is_ollama,
        ),
        ContextTier::Large => (
            0.80,
            0.40,
            20,
            vec![
                CompactionStrategy::Truncation,
                CompactionStrategy::ClientSummarization,
            ],
            !is_ollama,
        ),
    };

    // Tool clearing is always prepended (except Tiny which just truncates)
    let pipeline = if tier == ContextTier::Tiny {
        base_pipeline
    } else {
        let mut p = vec![CompactionStrategy::ToolClearing];
        p.extend(base_pipeline);
        p
    };

    // Apply user overrides
    CompactionProfile {
        tier,
        trigger_threshold: overrides.compaction_threshold.unwrap_or(base_threshold),
        target_after: base_target,
        recent_messages: overrides.recent_messages.unwrap_or(base_recent),
        pipeline,
        can_self_summarize: base_summarize,
    }
}

// ── Strategy functions ──────────────────────────────────────────────

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
/// Pinned messages are always preserved regardless of their position.
pub fn truncate_messages(
    messages: &[Message],
    keep_recent: usize,
    pinned_indices: &[usize],
) -> CompactionResult {
    if messages.len() <= keep_recent {
        return CompactionResult {
            messages: messages.to_vec(),
            summary: None,
            removed_count: 0,
        };
    }

    let split_point = messages.len() - keep_recent;
    let mut kept = Vec::new();
    let mut removed_count = 0;

    // Keep pinned messages from the older region
    for (i, msg) in messages[..split_point].iter().enumerate() {
        if pinned_indices.contains(&i) {
            kept.push(msg.clone());
        } else {
            removed_count += 1;
        }
    }

    // Always keep the recent window
    kept.extend_from_slice(&messages[split_point..]);

    CompactionResult {
        messages: kept,
        summary: Some(format!(
            "({removed_count} older messages were removed to fit context window)"
        )),
        removed_count,
    }
}

/// Build a structured summarization prompt from older messages.
///
/// The prompt instructs the LLM to preserve code blocks verbatim, list key decisions,
/// and retain file paths, variable names, and error messages.
pub fn build_summarization_prompt(
    messages_to_summarize: &[Message],
    pinned_previews: Option<&str>,
    custom_instructions: Option<&str>,
) -> String {
    let mut transcript = String::new();
    for msg in messages_to_summarize {
        let role = msg.role.as_str();
        transcript.push_str(&format!("{role}: {}\n", msg.content));
    }

    let mut prompt = format!(
        "You are summarizing a conversation to preserve continuity.\n\
         The original messages will be replaced by your summary.\n\
         \n\
         PRESERVE:\n\
         1. Code blocks — reproduce verbatim (fenced with ```)\n\
         2. Key decisions and their rationale — as bullet points\n\
         3. File paths, variable names, function names, error messages, URLs\n\
         4. Current state of work in progress\n\
         5. Unresolved questions or open items\n\
         6. Constraints or requirements stated by the user\n\
         \n\
         Write a structured summary. Keep it under 500 words.\n\
         \n\
         CONVERSATION:\n\
         {transcript}"
    );

    if let Some(pinned) = pinned_previews {
        prompt.push_str(&format!(
            "\n\nNote: The following messages are PINNED by the user and will be preserved \
             separately. Reference them but do not duplicate their content:\n{pinned}"
        ));
    }

    if let Some(instructions) = custom_instructions {
        prompt.push_str(&format!("\n\nAdditional focus: {instructions}"));
    }

    prompt
}

/// Apply client summarization: split messages into summarized and recent portions.
/// Returns a CompactionResult with a summarization prompt as the summary field.
/// The caller should send the prompt to the LLM to get the actual summary.
/// Pinned messages are preserved verbatim in the kept messages and referenced in the prompt.
pub fn prepare_client_summarization(
    messages: &[Message],
    keep_recent: usize,
    pinned_indices: &[usize],
    custom_instructions: Option<&str>,
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

    // Separate pinned messages from the older region
    let mut pinned_in_older = Vec::new();
    let mut non_pinned_to_summarize = Vec::new();
    for (i, msg) in to_summarize.iter().enumerate() {
        if pinned_indices.contains(&i) {
            pinned_in_older.push(msg.clone());
        } else {
            non_pinned_to_summarize.push(msg.clone());
        }
    }

    let pinned_previews = if pinned_in_older.is_empty() {
        None
    } else {
        let previews: Vec<String> = pinned_in_older
            .iter()
            .map(|m| {
                let preview: String = m.content.chars().take(100).collect();
                format!("- [{}]: {}", m.role.as_str(), preview)
            })
            .collect();
        Some(previews.join("\n"))
    };

    let messages_for_prompt = if pinned_in_older.is_empty() {
        to_summarize
    } else {
        &non_pinned_to_summarize
    };

    let prompt = build_summarization_prompt(
        messages_for_prompt,
        pinned_previews.as_deref(),
        custom_instructions,
    );

    let removed_count = non_pinned_to_summarize.len();

    // Kept = pinned older messages + recent window
    let mut kept = pinned_in_older;
    kept.extend_from_slice(to_keep);

    CompactionResult {
        messages: kept,
        summary: Some(prompt),
        removed_count,
    }
}

/// Result of clearing old tool results.
#[derive(Debug)]
pub struct ClearResult {
    /// Number of tool results that were cleared.
    pub cleared_count: usize,
    /// Estimated tokens reclaimed by clearing.
    pub tokens_reclaimed: u32,
}

/// Clear old tool results to reclaim context space without full compaction.
///
/// Replaces tool result content with "[tool result cleared]" for messages
/// older than the `keep_recent` most recent tool interactions.
/// Pinned messages are never modified.
pub fn clear_old_tool_results(
    messages: &mut Vec<Message>,
    keep_recent: usize,
    pinned_indices: &[usize],
) -> ClearResult {
    use crate::llm::capabilities::estimate_tokens;

    // Find indices of messages with tool content (tool results or tool calls)
    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.tool_call_id.is_some() || m.tool_calls.is_some())
        .map(|(i, _)| i)
        .collect();

    // Determine which tool messages are old enough to clear
    let clearable_count = tool_indices.len().saturating_sub(keep_recent);
    let to_clear = &tool_indices[..clearable_count];

    let mut cleared_count = 0;
    let mut tokens_reclaimed = 0u32;

    for &idx in to_clear {
        if pinned_indices.contains(&idx) {
            continue;
        }

        let msg = &mut messages[idx];
        let old_tokens = estimate_tokens(&msg.content);
        let placeholder = "[tool result cleared]";
        let new_tokens = estimate_tokens(placeholder);

        if old_tokens > new_tokens {
            tokens_reclaimed += old_tokens - new_tokens;
        }

        msg.content = placeholder.to_string();
        msg.tool_calls = None;
        msg.tool_call_id = None;
        cleared_count += 1;
    }

    ClearResult {
        cleared_count,
        tokens_reclaimed,
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

// ── Summarization execution ─────────────────────────────────────────

/// Execute a summarization prompt via an LLM provider's streaming chat method,
/// collecting all Delta chunks into a single summary string.
///
/// This avoids adding a non-streaming method to the `LlmProvider` trait.
pub async fn execute_summarization(
    provider: &dyn LlmProvider,
    model: &str,
    summarization_prompt: &str,
    max_summary_tokens: Option<u32>,
) -> Result<String, LlmError> {
    let request = ChatRequest::new(
        model,
        vec![
            Message::system("You are a summarization assistant. Produce only the summary."),
            Message::user(summarization_prompt),
        ],
    )
    .with_max_tokens(max_summary_tokens.unwrap_or(1024));

    let (tx, mut rx) = mpsc::unbounded_channel();
    provider.chat(request, tx).await?;

    let mut collected = String::new();
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(text) => collected.push_str(&text),
            StreamChunk::Done => break,
            StreamChunk::Error(e) => {
                return Err(LlmError::ApiError {
                    status: 0,
                    message: e,
                });
            }
            _ => {} // ignore Usage, ToolCallStart, etc.
        }
    }

    if collected.is_empty() {
        return Err(LlmError::ParseError(
            "Summarization returned empty response".to_string(),
        ));
    }

    Ok(collected)
}

// ── Compaction pipeline ─────────────────────────────────────────────

/// Result of running the full compaction pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    /// The compacted messages.
    pub messages: Vec<Message>,
    /// Summary text for context assembly (if summarization or truncation produced one).
    pub summary: Option<String>,
    /// Total messages removed across all pipeline steps.
    pub total_removed: usize,
    /// Total tokens reclaimed (primarily from tool clearing).
    pub total_tokens_reclaimed: u32,
    /// Which compaction strategies were actually applied, in order.
    pub steps_applied: Vec<CompactionMode>,
}

/// Run the compaction pipeline: try strategies in order, re-checking budget after each.
///
/// Stops early when context usage drops below `profile.target_after`.
/// If summarization fails (LLM error, empty response), falls back to truncation.
pub async fn run_compaction_pipeline(
    messages: &[Message],
    pinned_indices: &[usize],
    profile: &CompactionProfile,
    capabilities: &ModelCapabilities,
    provider: Option<(&dyn LlmProvider, &str)>,
    custom_instructions: Option<&str>,
) -> Result<PipelineResult, LlmError> {
    let target_tokens = (capabilities.context_window as f64 * profile.target_after) as u32;
    let mut msgs = messages.to_vec();
    let mut result = PipelineResult {
        messages: vec![],
        summary: None,
        total_removed: 0,
        total_tokens_reclaimed: 0,
        steps_applied: vec![],
    };

    for strategy in &profile.pipeline {
        // Budget check: are we already under target?
        let current_tokens = estimate_message_tokens(&msgs);
        if current_tokens <= target_tokens {
            break;
        }

        match strategy {
            CompactionStrategy::ToolClearing => {
                let clear_result =
                    clear_old_tool_results(&mut msgs, profile.recent_messages, pinned_indices);
                if clear_result.cleared_count > 0 {
                    result.total_tokens_reclaimed += clear_result.tokens_reclaimed;
                    result.steps_applied.push(CompactionMode::ToolClearing);
                }
            }
            CompactionStrategy::Truncation => {
                let trunc_result =
                    truncate_messages(&msgs, profile.recent_messages, pinned_indices);
                result.total_removed += trunc_result.removed_count;
                if trunc_result.summary.is_some() {
                    result.summary = trunc_result.summary;
                }
                msgs = trunc_result.messages;
                result.steps_applied.push(CompactionMode::Truncation);
            }
            CompactionStrategy::ClientSummarization => {
                let prep = prepare_client_summarization(
                    &msgs,
                    profile.recent_messages,
                    pinned_indices,
                    custom_instructions,
                );

                if prep.removed_count == 0 {
                    // Nothing to summarize
                    continue;
                }

                // Try LLM summarization if available
                let summarization_succeeded =
                    if let (Some((prov, model)), true) = (provider, profile.can_self_summarize) {
                        if let Some(ref prompt) = prep.summary {
                            match execute_summarization(prov, model, prompt, None).await {
                                Ok(summary_text) => {
                                    result.summary = Some(summary_text);
                                    result.total_removed += prep.removed_count;
                                    msgs = prep.messages;
                                    result.steps_applied.push(CompactionMode::Summarization);
                                    true
                                }
                                Err(e) => {
                                    tracing::warn!("Summarization failed, falling back to truncation: {e}");
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                // Fallback to truncation if summarization was unavailable or failed
                if !summarization_succeeded {
                    let trunc_result =
                        truncate_messages(&msgs, profile.recent_messages, pinned_indices);
                    result.total_removed += trunc_result.removed_count;
                    if trunc_result.summary.is_some() {
                        result.summary = trunc_result.summary;
                    }
                    msgs = trunc_result.messages;
                    result
                        .steps_applied
                        .push(CompactionMode::Truncation);
                }
            }
            CompactionStrategy::None => {}
        }
    }

    result.messages = msgs;
    Ok(result)
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

    /// Ensures truncation is a no-op when message count is already under the limit.
    #[test]
    fn truncate_no_op_when_under_limit() {
        let msgs = sample_messages(5);
        let result = truncate_messages(&msgs, 10, &[]);
        assert_eq!(result.messages.len(), 5);
        assert!(result.summary.is_none());
        assert_eq!(result.removed_count, 0);
    }

    /// Verifies truncation drops the oldest messages and keeps the newest ones.
    #[test]
    fn truncate_removes_oldest() {
        let msgs = sample_messages(20);
        let result = truncate_messages(&msgs, 10, &[]);
        assert_eq!(result.messages.len(), 10);
        assert_eq!(result.removed_count, 10);
        assert!(result.summary.is_some());
        assert_eq!(result.messages[0].content, "Question 10");
        assert_eq!(result.messages.last().unwrap().content, "Answer 19");
    }

    /// Verifies truncation preserves pinned messages in the older region.
    #[test]
    fn truncate_preserves_pinned() {
        let msgs = sample_messages(20);
        // Pin message at index 2 (in the older region, which is 0..10)
        let result = truncate_messages(&msgs, 10, &[2]);
        // 10 recent + 1 pinned = 11
        assert_eq!(result.messages.len(), 11);
        assert_eq!(result.removed_count, 9);
        assert_eq!(result.messages[0].content, "Question 2");
        assert_eq!(result.messages[1].content, "Question 10");
    }

    /// Verifies summarization splits messages and generates a prompt from the removed portion.
    #[test]
    fn prepare_summarization_splits_correctly() {
        let msgs = sample_messages(20);
        let result = prepare_client_summarization(&msgs, 10, &[], None);
        assert_eq!(result.messages.len(), 10);
        assert_eq!(result.removed_count, 10);
        let summary = result.summary.unwrap();
        assert!(summary.contains("PRESERVE"));
        assert!(summary.contains("Question 0"));
    }

    /// Ensures summarization is a no-op when message count is under the limit.
    #[test]
    fn prepare_summarization_no_op_when_under_limit() {
        let msgs = sample_messages(5);
        let result = prepare_client_summarization(&msgs, 10, &[], None);
        assert_eq!(result.messages.len(), 5);
        assert!(result.summary.is_none());
    }

    /// Verifies summarization preserves pinned messages and notes them in the prompt.
    #[test]
    fn prepare_summarization_preserves_pinned() {
        let msgs = sample_messages(20);
        let result = prepare_client_summarization(&msgs, 10, &[3], None);
        // 10 recent + 1 pinned = 11
        assert_eq!(result.messages.len(), 11);
        // Only 9 non-pinned messages removed (not the pinned one)
        assert_eq!(result.removed_count, 9);
        assert_eq!(result.messages[0].content, "Answer 3");
        let summary = result.summary.unwrap();
        assert!(summary.contains("PINNED"));
    }

    /// Validates the summarization prompt includes structured rules and content.
    #[test]
    fn build_summarization_prompt_format() {
        let msgs = vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
        ];
        let prompt = build_summarization_prompt(&msgs, None, None);
        assert!(prompt.contains("user: What is Rust?"));
        assert!(prompt.contains("assistant: Rust is a systems programming language."));
        assert!(prompt.contains("PRESERVE"));
        assert!(prompt.contains("Code blocks"));
        assert!(prompt.contains("Key decisions"));
    }

    /// Ensures Anthropic provider auto-selects summarization (supports caching).
    #[test]
    fn auto_select_for_anthropic() {
        assert_eq!(
            auto_select_strategy("anthropic"),
            CompactionStrategy::ClientSummarization
        );
    }

    /// Ensures Ollama provider auto-selects truncation (local, no caching).
    #[test]
    fn auto_select_for_ollama() {
        assert_eq!(
            auto_select_strategy("ollama"),
            CompactionStrategy::Truncation
        );
    }

    /// Validates parsing of all strategy string variants including unknown fallback.
    #[test]
    fn strategy_from_str() {
        assert_eq!(CompactionStrategy::from_str("none"), CompactionStrategy::None);
        assert_eq!(CompactionStrategy::from_str("tool_clearing"), CompactionStrategy::ToolClearing);
        assert_eq!(CompactionStrategy::from_str("truncation"), CompactionStrategy::Truncation);
        assert_eq!(CompactionStrategy::from_str("summarize"), CompactionStrategy::ClientSummarization);
        assert_eq!(CompactionStrategy::from_str("unknown"), CompactionStrategy::Truncation);
    }

    /// Ensures strategy serialization and parsing are inverse operations.
    #[test]
    fn strategy_roundtrip() {
        for s in &[
            CompactionStrategy::None,
            CompactionStrategy::ToolClearing,
            CompactionStrategy::Truncation,
            CompactionStrategy::ClientSummarization,
        ] {
            assert_eq!(CompactionStrategy::from_str(s.as_str()), *s);
        }
    }

    /// Verifies clear_old_tool_results clears old tool messages but keeps recent ones.
    #[test]
    fn clear_old_tool_results_clears_old() {
        use crate::llm::types::{Role, ToolCall};

        let mut msgs = vec![
            Message::user("call tool A"),
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "t1".into(),
                    name: "toolA".into(),
                    arguments: "{}".into(),
                }]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "result A with lots of data".into(),
                tool_calls: None,
                tool_call_id: Some("t1".into()),
            },
            Message::user("call tool B"),
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "t2".into(),
                    name: "toolB".into(),
                    arguments: "{}".into(),
                }]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "result B with lots of data".into(),
                tool_calls: None,
                tool_call_id: Some("t2".into()),
            },
        ];

        // Keep only the 2 most recent tool messages, clear the rest
        let result = clear_old_tool_results(&mut msgs, 2, &[]);
        // The 2 oldest tool-related messages (indices 1,2) should be cleared
        // The 2 newest (indices 4,5) are kept
        assert!(result.cleared_count > 0);
        assert!(result.tokens_reclaimed > 0);
        assert_eq!(msgs[2].content, "[tool result cleared]");
        assert_eq!(msgs[5].content, "result B with lots of data");
    }

    /// Verifies clear_old_tool_results preserves pinned tool messages.
    #[test]
    fn clear_old_tool_results_preserves_pinned() {
        use crate::llm::types::{Role, ToolCall};

        let mut msgs = vec![
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "t1".into(),
                    name: "toolA".into(),
                    arguments: "{}".into(),
                }]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "important pinned result".into(),
                tool_calls: None,
                tool_call_id: Some("t1".into()),
            },
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "t2".into(),
                    name: "toolB".into(),
                    arguments: "{}".into(),
                }]),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "result B".into(),
                tool_calls: None,
                tool_call_id: Some("t2".into()),
            },
        ];

        // Pin index 1 (the first tool result), keep_recent=1
        let result = clear_old_tool_results(&mut msgs, 1, &[1]);
        // Index 0 (assistant with tool_calls) should be cleared
        // Index 1 is pinned, should NOT be cleared
        assert_eq!(msgs[1].content, "important pinned result");
        // Only the non-pinned old tool messages get cleared
        assert!(result.cleared_count <= 3);
    }

    // ── CompactionProfile tests ─────────────────────────────────────

    #[test]
    fn context_tier_from_window_tiny() {
        assert_eq!(ContextTier::from_context_window(0), ContextTier::Tiny);
        assert_eq!(ContextTier::from_context_window(4_000), ContextTier::Tiny);
        assert_eq!(ContextTier::from_context_window(8_000), ContextTier::Tiny);
    }

    #[test]
    fn context_tier_from_window_small() {
        assert_eq!(ContextTier::from_context_window(8_001), ContextTier::Small);
        assert_eq!(ContextTier::from_context_window(16_000), ContextTier::Small);
        assert_eq!(ContextTier::from_context_window(32_000), ContextTier::Small);
    }

    #[test]
    fn context_tier_from_window_medium() {
        assert_eq!(ContextTier::from_context_window(32_001), ContextTier::Medium);
        assert_eq!(ContextTier::from_context_window(128_000), ContextTier::Medium);
        assert_eq!(ContextTier::from_context_window(200_000), ContextTier::Medium);
    }

    #[test]
    fn context_tier_from_window_large() {
        assert_eq!(ContextTier::from_context_window(200_001), ContextTier::Large);
        assert_eq!(ContextTier::from_context_window(1_000_000), ContextTier::Large);
    }

    #[test]
    fn derive_profile_tiny_model() {
        let caps = ModelCapabilities { context_window: 4_000, ..Default::default() };
        let profile = derive_profile(&caps, "ollama", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Tiny);
        assert!((profile.trigger_threshold - 0.50).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 4);
        assert_eq!(profile.pipeline, vec![CompactionStrategy::Truncation]);
        assert!(!profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_small_model() {
        let caps = ModelCapabilities { context_window: 16_000, ..Default::default() };
        let profile = derive_profile(&caps, "openai", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Small);
        assert!((profile.trigger_threshold - 0.60).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 8);
        assert!(profile.pipeline.contains(&CompactionStrategy::ToolClearing));
        assert!(profile.pipeline.contains(&CompactionStrategy::Truncation));
        assert!(!profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_medium_openai() {
        let caps = ModelCapabilities { context_window: 128_000, ..Default::default() };
        let profile = derive_profile(&caps, "openai", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Medium);
        assert!((profile.trigger_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 15);
        assert!(profile.can_self_summarize);
        assert!(profile.pipeline.contains(&CompactionStrategy::ToolClearing));
        assert!(profile.pipeline.contains(&CompactionStrategy::ClientSummarization));
    }

    #[test]
    fn derive_profile_medium_ollama_no_self_summarize() {
        let caps = ModelCapabilities { context_window: 128_000, ..Default::default() };
        let profile = derive_profile(&caps, "ollama", &ProfileOverrides::default());
        assert!(!profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_large_model() {
        let caps = ModelCapabilities { context_window: 200_001, ..Default::default() };
        let profile = derive_profile(&caps, "anthropic", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Large);
        assert!((profile.trigger_threshold - 0.80).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 20);
        assert!(profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_overrides_win() {
        let caps = ModelCapabilities { context_window: 128_000, ..Default::default() };
        let overrides = ProfileOverrides {
            compaction_threshold: Some(0.90),
            recent_messages: Some(30),
            compaction_strategy: None,
        };
        let profile = derive_profile(&caps, "openai", &overrides);
        assert!((profile.trigger_threshold - 0.90).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 30);
    }

    #[test]
    fn derive_profile_none_strategy_disables_pipeline() {
        let caps = ModelCapabilities { context_window: 128_000, ..Default::default() };
        let overrides = ProfileOverrides {
            compaction_strategy: Some("none".to_string()),
            ..Default::default()
        };
        let profile = derive_profile(&caps, "openai", &overrides);
        assert!(profile.pipeline.is_empty());
        assert!((profile.trigger_threshold - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn profile_overrides_from_config_defaults_are_none() {
        let config = ConversationConfig::default();
        let overrides = ProfileOverrides::from_config(&config);
        assert!(overrides.compaction_threshold.is_none());
        assert!(overrides.recent_messages.is_none());
        assert!(overrides.compaction_strategy.is_none());
    }

    #[test]
    fn profile_overrides_from_config_detects_changes() {
        let config = ConversationConfig {
            compaction_threshold: 0.60,
            recent_messages: 30,
            compaction_strategy: "truncation".to_string(),
            ..Default::default()
        };
        let overrides = ProfileOverrides::from_config(&config);
        assert_eq!(overrides.compaction_threshold, Some(0.60));
        assert_eq!(overrides.recent_messages, Some(30));
        assert_eq!(overrides.compaction_strategy.as_deref(), Some("truncation"));
    }

    // ── Improved prompt tests ───────────────────────────────────────

    #[test]
    fn build_prompt_includes_custom_instructions() {
        let msgs = vec![Message::user("hello")];
        let prompt = build_summarization_prompt(&msgs, None, Some("preserve auth decisions"));
        assert!(prompt.contains("Additional focus: preserve auth decisions"));
    }

    #[test]
    fn build_prompt_includes_pinned_previews() {
        let msgs = vec![Message::user("hello")];
        let prompt = build_summarization_prompt(&msgs, Some("- [user]: important msg"), None);
        assert!(prompt.contains("PINNED"));
        assert!(prompt.contains("important msg"));
    }

    #[test]
    fn build_prompt_omits_optional_sections_when_none() {
        let msgs = vec![Message::user("hello")];
        let prompt = build_summarization_prompt(&msgs, None, None);
        assert!(!prompt.contains("PINNED"));
        assert!(!prompt.contains("Additional focus"));
    }

    #[test]
    fn prepare_summarization_threads_custom_instructions() {
        let msgs = sample_messages(20);
        let result = prepare_client_summarization(&msgs, 10, &[], Some("keep the error logs"));
        let summary = result.summary.unwrap();
        assert!(summary.contains("keep the error logs"));
    }

    // ── execute_summarization tests ─────────────────────────────────

    use async_trait::async_trait;
    use crate::llm::types::ModelInfo;

    struct MockSummarizerProvider {
        response_chunks: Vec<StreamChunk>,
    }

    #[async_trait]
    impl LlmProvider for MockSummarizerProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn available_models(&self) -> Vec<ModelInfo> {
            vec![]
        }

        async fn chat(
            &self,
            _request: ChatRequest,
            tx: mpsc::UnboundedSender<StreamChunk>,
        ) -> Result<(), LlmError> {
            for chunk in &self.response_chunks {
                tx.send(chunk.clone()).ok();
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn execute_summarization_collects_deltas() {
        let provider = MockSummarizerProvider {
            response_chunks: vec![
                StreamChunk::Delta("The conversation ".to_string()),
                StreamChunk::Delta("discussed Rust.".to_string()),
                StreamChunk::Done,
            ],
        };
        let result = execute_summarization(&provider, "test-model", "summarize this", None).await;
        assert_eq!(result.unwrap(), "The conversation discussed Rust.");
    }

    #[tokio::test]
    async fn execute_summarization_returns_error_on_stream_error() {
        let provider = MockSummarizerProvider {
            response_chunks: vec![
                StreamChunk::Delta("partial".to_string()),
                StreamChunk::Error("API rate limit".to_string()),
            ],
        };
        let result = execute_summarization(&provider, "test-model", "summarize", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_summarization_returns_error_on_empty_response() {
        let provider = MockSummarizerProvider {
            response_chunks: vec![StreamChunk::Done],
        };
        let result = execute_summarization(&provider, "test-model", "summarize", None).await;
        assert!(result.is_err());
    }

    // ── Pipeline tests ──────────────────────────────────────────────

    /// Helper to create a large conversation that exceeds target budget.
    fn large_conversation(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| {
                // Each message ~50 chars → ~13 tokens
                Message::user(format!("This is message number {i:04} with some extra padding text"))
            })
            .collect()
    }

    #[tokio::test]
    async fn pipeline_exits_early_when_under_target() {
        // Small conversation that's already under any reasonable target
        let msgs = sample_messages(4);
        let caps = ModelCapabilities { context_window: 100_000, ..Default::default() };
        let profile = CompactionProfile {
            tier: ContextTier::Medium,
            trigger_threshold: 0.70,
            target_after: 0.30,
            recent_messages: 10,
            pipeline: vec![
                CompactionStrategy::ToolClearing,
                CompactionStrategy::Truncation,
            ],
            can_self_summarize: false,
        };
        let result = run_compaction_pipeline(&msgs, &[], &profile, &caps, None, None)
            .await
            .unwrap();
        // Should exit immediately — nothing to compact
        assert!(result.steps_applied.is_empty());
        assert_eq!(result.messages.len(), 4);
    }

    #[tokio::test]
    async fn pipeline_applies_truncation() {
        // 40 messages with a tiny context window → needs compaction
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        let profile = CompactionProfile {
            tier: ContextTier::Small,
            trigger_threshold: 0.60,
            target_after: 0.25,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::Truncation],
            can_self_summarize: false,
        };
        let result = run_compaction_pipeline(&msgs, &[], &profile, &caps, None, None)
            .await
            .unwrap();
        assert!(result.steps_applied.contains(&CompactionMode::Truncation));
        assert!(result.total_removed > 0);
        assert_eq!(result.messages.len(), 5); // recent_messages
    }

    #[tokio::test]
    async fn pipeline_falls_back_to_truncation_when_no_provider() {
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        let profile = CompactionProfile {
            tier: ContextTier::Medium,
            trigger_threshold: 0.70,
            target_after: 0.30,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::ClientSummarization],
            can_self_summarize: true,
        };
        // No provider → should fall back to truncation
        let result = run_compaction_pipeline(&msgs, &[], &profile, &caps, None, None)
            .await
            .unwrap();
        assert!(result.steps_applied.contains(&CompactionMode::Truncation));
        assert!(result.total_removed > 0);
    }

    #[tokio::test]
    async fn pipeline_falls_back_on_summarization_error() {
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        // Provider that returns an error
        let error_provider = MockSummarizerProvider {
            response_chunks: vec![StreamChunk::Error("API down".to_string())],
        };
        let profile = CompactionProfile {
            tier: ContextTier::Medium,
            trigger_threshold: 0.70,
            target_after: 0.30,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::ClientSummarization],
            can_self_summarize: true,
        };
        let result = run_compaction_pipeline(
            &msgs,
            &[],
            &profile,
            &caps,
            Some((&error_provider as &dyn LlmProvider, "model")),
            None,
        )
        .await
        .unwrap();
        // Should have fallen back to truncation
        assert!(result.steps_applied.contains(&CompactionMode::Truncation));
    }

    #[tokio::test]
    async fn pipeline_successful_summarization() {
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        let provider = MockSummarizerProvider {
            response_chunks: vec![
                StreamChunk::Delta("Summary of the conversation.".to_string()),
                StreamChunk::Done,
            ],
        };
        let profile = CompactionProfile {
            tier: ContextTier::Medium,
            trigger_threshold: 0.70,
            target_after: 0.30,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::ClientSummarization],
            can_self_summarize: true,
        };
        let result = run_compaction_pipeline(
            &msgs,
            &[],
            &profile,
            &caps,
            Some((&provider as &dyn LlmProvider, "model")),
            None,
        )
        .await
        .unwrap();
        assert!(result.steps_applied.contains(&CompactionMode::Summarization));
        assert_eq!(result.summary.as_deref(), Some("Summary of the conversation."));
    }

    #[tokio::test]
    async fn pipeline_preserves_pinned_messages() {
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        let profile = CompactionProfile {
            tier: ContextTier::Small,
            trigger_threshold: 0.60,
            target_after: 0.25,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::Truncation],
            can_self_summarize: false,
        };
        // Pin message at index 2
        let result = run_compaction_pipeline(&msgs, &[2], &profile, &caps, None, None)
            .await
            .unwrap();
        // The pinned message should survive
        assert!(
            result.messages.iter().any(|m| m.content == msgs[2].content),
            "Pinned message at index 2 should survive compaction"
        );
    }

    #[tokio::test]
    async fn pipeline_threads_custom_instructions() {
        let msgs = large_conversation(40);
        let caps = ModelCapabilities { context_window: 200, ..Default::default() };
        let provider = MockSummarizerProvider {
            response_chunks: vec![
                StreamChunk::Delta("Summary with auth focus.".to_string()),
                StreamChunk::Done,
            ],
        };
        let profile = CompactionProfile {
            tier: ContextTier::Medium,
            trigger_threshold: 0.70,
            target_after: 0.30,
            recent_messages: 5,
            pipeline: vec![CompactionStrategy::ClientSummarization],
            can_self_summarize: true,
        };
        let result = run_compaction_pipeline(
            &msgs,
            &[],
            &profile,
            &caps,
            Some((&provider as &dyn LlmProvider, "model")),
            Some("preserve auth decisions"),
        )
        .await
        .unwrap();
        // Summarization should succeed (we can't easily verify the prompt content
        // was threaded, but we verify the pipeline accepted custom_instructions)
        assert!(result.steps_applied.contains(&CompactionMode::Summarization));
    }
}
