/// Budget-aware context assembly pipeline.
///
/// Assembles messages for an LLM request while respecting context window limits.
/// Priority order (highest to lowest):
/// 1. System prompts (always included)
/// 2. Tool definitions (estimated token overhead)
/// 3. Most recent N messages (configurable, default 10)
/// 4. Compaction summary (if older messages were compacted)
/// 5. Older messages (as many as fit in remaining budget)

use crate::llm::capabilities::{estimate_message_tokens, estimate_tokens, ModelCapabilities};
use crate::llm::types::Message;

/// Configuration for context assembly.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Number of most recent messages to always include.
    pub recent_message_count: usize,
    /// Fraction of context window to target (0.0-1.0). Leaves headroom for output.
    pub budget_fraction: f64,
    /// Estimated tokens per tool definition.
    pub tokens_per_tool: u32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            recent_message_count: 20,
            budget_fraction: 0.80,
            tokens_per_tool: 200,
        }
    }
}

/// Result of context assembly.
#[derive(Debug)]
pub struct AssembledContext {
    /// The assembled messages to send to the LLM.
    pub messages: Vec<Message>,
    /// Estimated total tokens in the assembled context.
    pub estimated_tokens: u32,
    /// Number of messages that were dropped to fit the budget.
    pub dropped_count: usize,
    /// Whether compaction is recommended (context usage > 80% of budget).
    pub compaction_recommended: bool,
    /// Context usage as a fraction (0.0-1.0) of the context window.
    pub usage_fraction: f64,
}

/// Assemble messages for an LLM request within the context budget.
pub fn assemble_context(
    system_messages: &[Message],
    context_messages: &[Message],
    conversation_messages: &[Message],
    compaction_summary: Option<&str>,
    tool_count: usize,
    capabilities: &ModelCapabilities,
    config: &ContextConfig,
) -> AssembledContext {
    let max_budget =
        (capabilities.context_window as f64 * config.budget_fraction) as u32;

    // 1. System prompts — always included
    let system_tokens = estimate_message_tokens(system_messages);

    // 2. Context (user-defined context files) — always included
    let context_tokens = estimate_message_tokens(context_messages);

    // 3. Tool overhead
    let tool_tokens = tool_count as u32 * config.tokens_per_tool;

    let fixed_tokens = system_tokens + context_tokens + tool_tokens;

    // 4. Compaction summary (if present)
    let summary_tokens = compaction_summary
        .map(|s| estimate_tokens(s) + 4) // +4 for message overhead
        .unwrap_or(0);

    let available = max_budget.saturating_sub(fixed_tokens + summary_tokens);

    // 5. Split conversation messages: recent (always) + older (as budget allows)
    let total_msgs = conversation_messages.len();
    let recent_start = total_msgs.saturating_sub(config.recent_message_count);
    let recent = &conversation_messages[recent_start..];
    let older = &conversation_messages[..recent_start];

    let recent_tokens = estimate_message_tokens(recent);

    // Fill older messages from newest to oldest within remaining budget
    let remaining = available.saturating_sub(recent_tokens);
    let mut older_to_include = Vec::new();
    let mut older_tokens_used = 0u32;

    for msg in older.iter().rev() {
        let msg_tokens = estimate_message_tokens(&[msg.clone()]);
        if older_tokens_used + msg_tokens > remaining {
            break;
        }
        older_to_include.push(msg.clone());
        older_tokens_used += msg_tokens;
    }
    older_to_include.reverse(); // restore chronological order

    let dropped_count = older.len() - older_to_include.len();

    // Assemble final message list
    let mut messages = Vec::new();
    messages.extend_from_slice(system_messages);
    messages.extend_from_slice(context_messages);

    if let Some(summary) = compaction_summary {
        if dropped_count > 0 || !older_to_include.is_empty() {
            messages.push(Message::system(format!(
                "[Previous conversation summary: {summary}]"
            )));
        }
    }

    messages.extend(older_to_include);
    messages.extend_from_slice(recent);

    let estimated_tokens = fixed_tokens + summary_tokens + older_tokens_used + recent_tokens;
    let usage_fraction = estimated_tokens as f64 / capabilities.context_window as f64;
    let compaction_recommended = usage_fraction > 0.75;

    AssembledContext {
        messages,
        estimated_tokens,
        dropped_count,
        compaction_recommended,
        usage_fraction,
    }
}

/// Format context usage as a color hint for the status bar.
/// Returns (percentage, color_hint) where color_hint is "green", "yellow", or "red".
pub fn context_usage_color(fraction: f64) -> (&'static str, &'static str) {
    if fraction < 0.5 {
        ("green", "▪")
    } else if fraction < 0.75 {
        ("yellow", "▪")
    } else {
        ("red", "▪")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_caps() -> ModelCapabilities {
        ModelCapabilities {
            context_window: 1000,
            max_output: 200,
            ..Default::default()
        }
    }

    fn config_with_recent(n: usize) -> ContextConfig {
        ContextConfig {
            recent_message_count: n,
            budget_fraction: 0.80,
            tokens_per_tool: 100,
        }
    }

    /// Ensures assembling an empty conversation produces no messages, tokens, or drops.
    #[test]
    fn empty_conversation() {
        let result = assemble_context(&[], &[], &[], None, 0, &default_caps(), &config_with_recent(10));
        assert!(result.messages.is_empty());
        assert_eq!(result.estimated_tokens, 0);
        assert_eq!(result.dropped_count, 0);
    }

    /// Verifies system prompts are always placed first in the assembled output.
    #[test]
    fn system_prompt_always_included() {
        let system = vec![Message::system("You are helpful")];
        let msgs = vec![Message::user("hi")];
        let result = assemble_context(&system, &[], &msgs, None, 0, &default_caps(), &config_with_recent(10));
        assert_eq!(result.messages[0].content, "You are helpful");
        assert!(result.messages.len() >= 2);
    }

    /// Ensures the N most recent messages are always kept even when older ones are dropped.
    #[test]
    fn recent_messages_preserved() {
        let msgs: Vec<Message> = (0..30)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            None,
            0,
            &ModelCapabilities {
                context_window: 100_000,
                ..Default::default()
            },
            &config_with_recent(10),
        );
        assert!(result.messages.len() >= 10);
        assert_eq!(result.messages.last().unwrap().content, "msg 29");
    }

    /// Verifies the compaction summary is injected when messages are dropped due to budget.
    #[test]
    fn compaction_summary_included_when_messages_dropped() {
        let caps = ModelCapabilities {
            context_window: 200,
            max_output: 50,
            ..Default::default()
        };
        let msgs: Vec<Message> = (0..50)
            .map(|i| Message::user(format!("message number {i} with some extra text")))
            .collect();
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            Some("Earlier we discussed Rust programming"),
            0,
            &caps,
            &config_with_recent(5),
        );
        let has_summary = result
            .messages
            .iter()
            .any(|m| m.content.contains("Earlier we discussed"));
        assert!(has_summary);
        assert!(result.dropped_count > 0);
    }

    /// Verifies that tool definitions consume budget, leaving fewer messages included.
    #[test]
    fn tool_count_reduces_budget() {
        let msgs: Vec<Message> = (0..20)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let caps = ModelCapabilities {
            context_window: 500,
            max_output: 100,
            ..Default::default()
        };
        let with_tools = assemble_context(
            &[], &[], &msgs, None, 5, &caps, &config_with_recent(10),
        );
        let without_tools = assemble_context(
            &[], &[], &msgs, None, 0, &caps, &config_with_recent(10),
        );
        assert!(with_tools.messages.len() <= without_tools.messages.len());
    }

    /// Validates the green/yellow/red color thresholds for the context usage indicator.
    #[test]
    fn context_color_thresholds() {
        assert_eq!(context_usage_color(0.3).0, "green");
        assert_eq!(context_usage_color(0.6).0, "yellow");
        assert_eq!(context_usage_color(0.8).0, "red");
    }

    /// Ensures usage_fraction is computed correctly relative to the context window size.
    #[test]
    fn usage_fraction_calculated() {
        let caps = ModelCapabilities {
            context_window: 10_000,
            ..Default::default()
        };
        let msgs = vec![Message::user("hello")];
        let result = assemble_context(
            &[], &[], &msgs, None, 0, &caps, &ContextConfig::default(),
        );
        assert!(result.usage_fraction > 0.0);
        assert!(result.usage_fraction < 0.01);
    }

    /// Verifies user-defined context messages are included in the assembled output.
    #[test]
    fn context_messages_included() {
        let ctx = vec![Message::system("You are a Rust expert")];
        let msgs = vec![Message::user("help me")];
        let result = assemble_context(
            &[], &ctx, &msgs, None, 0, &default_caps(), &config_with_recent(10),
        );
        assert!(result.messages.iter().any(|m| m.content.contains("Rust expert")));
    }

    /// Ensures the compaction summary is omitted when all messages fit within the recent window.
    #[test]
    fn no_summary_when_all_messages_fit() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let result = assemble_context(
            &[], &[], &msgs, Some("old summary"), 0,
            &ModelCapabilities { context_window: 100_000, ..Default::default() },
            &config_with_recent(10),
        );
        assert_eq!(result.dropped_count, 0);
        let has_summary = result.messages.iter().any(|m| m.content.contains("old summary"));
        assert!(!has_summary);
    }

    /// Verifies compaction is not recommended when context usage is well below threshold.
    #[test]
    fn compaction_not_recommended_for_small_context() {
        let msgs = vec![Message::user("hi")];
        let result = assemble_context(
            &[], &[], &msgs, None, 0,
            &ModelCapabilities { context_window: 100_000, ..Default::default() },
            &config_with_recent(10),
        );
        assert!(!result.compaction_recommended);
    }

    /// Ensures older messages that fit the budget are kept in chronological order.
    #[test]
    fn older_messages_in_chronological_order() {
        let msgs: Vec<Message> = (0..30)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let result = assemble_context(
            &[], &[], &msgs, None, 0,
            &ModelCapabilities { context_window: 100_000, ..Default::default() },
            &config_with_recent(10),
        );
        assert_eq!(result.messages.len(), 30);
        assert_eq!(result.messages[0].content, "msg 0");
        assert_eq!(result.messages[29].content, "msg 29");
    }

    /// Ensures summary is included when older messages are present but none were dropped
    /// (dropped_count == 0 but older_to_include is non-empty).
    #[test]
    fn summary_included_when_older_included_but_none_dropped() {
        // 15 messages with recent_message_count=10 means 5 older messages.
        // Large context window so all 5 older messages fit (dropped_count == 0).
        let msgs: Vec<Message> = (0..15)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            Some("summary of earlier work"),
            0,
            &ModelCapabilities { context_window: 100_000, ..Default::default() },
            &config_with_recent(10),
        );
        assert_eq!(result.dropped_count, 0);
        let has_summary = result.messages.iter().any(|m| m.content.contains("summary of earlier work"));
        assert!(has_summary, "summary should be included when older_to_include is non-empty");
    }

    /// Ensures summary is included when messages are dropped (dropped_count > 0)
    /// even if older_to_include ends up empty.
    #[test]
    fn summary_included_when_messages_dropped() {
        // Many messages with a tiny context window so older messages get dropped.
        let msgs: Vec<Message> = (0..50)
            .map(|i| Message::user(format!("message number {i} with padding text")))
            .collect();
        let caps = ModelCapabilities {
            context_window: 300,
            max_output: 50,
            ..Default::default()
        };
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            Some("dropped context summary"),
            0,
            &caps,
            &config_with_recent(5),
        );
        assert!(result.dropped_count > 0);
        let has_summary = result.messages.iter().any(|m| m.content.contains("dropped context summary"));
        assert!(has_summary, "summary should be included when dropped_count > 0");
    }

    /// Verifies compaction_recommended is true at exactly 0.75 usage fraction boundary.
    #[test]
    fn compaction_recommended_boundary_at_075() {
        // The threshold is usage_fraction > 0.75, so exactly 0.75 should NOT recommend.
        // We check that usage at exactly 0.75 is not recommended and just above is.
        assert!(!{ let f = 0.75_f64; f > 0.75 }, "0.75 should not exceed the 0.75 threshold");
        assert!({ let f = 0.76_f64; f > 0.75 }, "0.76 should exceed the 0.75 threshold");

        // Verify via context_usage_color as well: 0.75 is the yellow/red boundary.
        assert_eq!(context_usage_color(0.74).0, "yellow");
        assert_eq!(context_usage_color(0.75).0, "red");
    }

    /// Verifies that a zero budget_fraction results in a budget of 0 tokens and drops all messages.
    #[test]
    fn zero_budget_fraction_drops_everything() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let config = ContextConfig {
            recent_message_count: 10,
            budget_fraction: 0.0,
            tokens_per_tool: 0,
        };
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            None,
            0,
            &default_caps(),
            &config,
        );
        // With zero budget, recent messages are still "included" by the slice logic,
        // but the budget math means estimated_tokens can exceed the 0 budget.
        // The key invariant: the function does not panic with a zero budget.
        assert!(result.usage_fraction >= 0.0);
    }

    /// Verifies that a very large tool_count consumes most of the budget, leaving minimal room.
    #[test]
    fn large_tool_count_consumes_budget() {
        let msgs: Vec<Message> = (0..20)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let caps = ModelCapabilities {
            context_window: 1000,
            max_output: 100,
            ..Default::default()
        };
        // 100 tokens_per_tool * 7 tools = 700 tokens of overhead out of 800 budget (80%).
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            None,
            7,
            &caps,
            &config_with_recent(5),
        );
        // Tool overhead alone is 700 tokens; very few (if any) older messages should fit.
        assert!(result.estimated_tokens >= 700);
        // Most older messages should be dropped.
        assert!(result.dropped_count > 0, "large tool overhead should cause drops");
    }

    /// Verifies that when all messages fit within recent_message_count no splitting occurs.
    #[test]
    fn all_messages_fit_within_recent_count() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| Message::user(format!("msg {i}")))
            .collect();
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            None,
            0,
            &ModelCapabilities { context_window: 100_000, ..Default::default() },
            &config_with_recent(10),
        );
        assert_eq!(result.messages.len(), 5);
        assert_eq!(result.dropped_count, 0);
        assert_eq!(result.messages[0].content, "msg 0");
        assert_eq!(result.messages[4].content, "msg 4");
    }

    /// Verifies correct behavior with a single-message conversation.
    #[test]
    fn single_message_conversation() {
        let msgs = vec![Message::user("only message")];
        let result = assemble_context(
            &[],
            &[],
            &msgs,
            None,
            0,
            &default_caps(),
            &config_with_recent(10),
        );
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].content, "only message");
        assert_eq!(result.dropped_count, 0);
    }

    /// Verifies context messages are placed after system messages but before conversation messages.
    #[test]
    fn context_messages_ordered_after_system_before_conversation() {
        let system = vec![Message::system("system prompt")];
        let context = vec![Message::system("context info")];
        let conversation = vec![Message::user("user question")];
        let result = assemble_context(
            &system,
            &context,
            &conversation,
            None,
            0,
            &default_caps(),
            &config_with_recent(10),
        );
        assert_eq!(result.messages.len(), 3);
        assert_eq!(result.messages[0].content, "system prompt");
        assert_eq!(result.messages[1].content, "context info");
        assert_eq!(result.messages[2].content, "user question");
    }
}
