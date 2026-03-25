/// Compaction strategies for managing long conversations.
///
/// When a conversation approaches the context window limit, compaction
/// summarizes or truncates older messages to free up space.

mod pipeline;
mod strategy;

// Re-export everything at the crate::llm::compaction level so callers are unchanged.
pub use pipeline::{
    build_summarization_prompt, clear_old_tool_results, execute_summarization,
    prepare_client_summarization, run_compaction_pipeline, truncate_messages, ClearResult,
    CompactionResult, PipelineResult,
};
pub use strategy::{
    auto_select_strategy, derive_profile, CompactionProfile, CompactionStrategy, ContextTier,
    ProfileOverrides,
};
