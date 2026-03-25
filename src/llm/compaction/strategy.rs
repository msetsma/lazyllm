/// Compaction strategy definitions, tier classification, and profile derivation.

use crate::config::types::ConversationConfig;
use crate::llm::capabilities::ModelCapabilities;

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
            vec![CompactionStrategy::ClientSummarization],
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
    use crate::config::types::ConversationConfig;

    #[test]
    fn strategy_from_str() {
        assert_eq!(CompactionStrategy::from_str("none"), CompactionStrategy::None);
        assert_eq!(
            CompactionStrategy::from_str("tool_clearing"),
            CompactionStrategy::ToolClearing
        );
        assert_eq!(
            CompactionStrategy::from_str("truncation"),
            CompactionStrategy::Truncation
        );
        assert_eq!(
            CompactionStrategy::from_str("summarize"),
            CompactionStrategy::ClientSummarization
        );
        assert_eq!(
            CompactionStrategy::from_str("unknown"),
            CompactionStrategy::Truncation
        );
    }

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
        assert_eq!(
            ContextTier::from_context_window(1_000_000),
            ContextTier::Large
        );
    }

    #[test]
    fn derive_profile_tiny_model() {
        let caps = ModelCapabilities {
            context_window: 4_000,
            ..Default::default()
        };
        let profile = derive_profile(&caps, "ollama", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Tiny);
        assert!((profile.trigger_threshold - 0.50).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 4);
        assert_eq!(profile.pipeline, vec![CompactionStrategy::Truncation]);
        assert!(!profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_small_model() {
        let caps = ModelCapabilities {
            context_window: 16_000,
            ..Default::default()
        };
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
        let caps = ModelCapabilities {
            context_window: 128_000,
            ..Default::default()
        };
        let profile = derive_profile(&caps, "openai", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Medium);
        assert!((profile.trigger_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 15);
        assert!(profile.can_self_summarize);
        assert!(profile.pipeline.contains(&CompactionStrategy::ToolClearing));
        assert!(profile
            .pipeline
            .contains(&CompactionStrategy::ClientSummarization));
    }

    #[test]
    fn derive_profile_medium_ollama_no_self_summarize() {
        let caps = ModelCapabilities {
            context_window: 128_000,
            ..Default::default()
        };
        let profile = derive_profile(&caps, "ollama", &ProfileOverrides::default());
        assert!(!profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_large_model() {
        let caps = ModelCapabilities {
            context_window: 200_001,
            ..Default::default()
        };
        let profile = derive_profile(&caps, "anthropic", &ProfileOverrides::default());
        assert_eq!(profile.tier, ContextTier::Large);
        assert!((profile.trigger_threshold - 0.80).abs() < f64::EPSILON);
        assert_eq!(profile.recent_messages, 20);
        assert!(profile.can_self_summarize);
    }

    #[test]
    fn derive_profile_overrides_win() {
        let caps = ModelCapabilities {
            context_window: 128_000,
            ..Default::default()
        };
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
        let caps = ModelCapabilities {
            context_window: 128_000,
            ..Default::default()
        };
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
        assert_eq!(
            overrides.compaction_strategy.as_deref(),
            Some("truncation")
        );
    }
}
