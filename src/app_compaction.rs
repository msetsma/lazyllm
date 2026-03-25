/// Compaction orchestration methods for [`App`].
///
/// Handles spawning the async compaction pipeline, draining results,
/// auto-trigger heuristics, and refreshing the Pulse overlay data.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::app::App;
use crate::llm::compaction::{self, PipelineResult, ProfileOverrides};
use crate::llm::capabilities;
use crate::ui::components::chat_view::{ChatMessage, MessageRole};

impl App {
    /// Spawn an async compaction pipeline for the active conversation.
    pub(crate) fn spawn_compaction(&mut self, custom_instructions: Option<String>) {
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => {
                self.status_bar
                    .set_status("No active conversation".to_string());
                return;
            }
        };

        if self.compaction_rx.is_some() {
            self.status_bar
                .set_status("Compaction already in progress".to_string());
            return;
        }

        let provider_name = &self.config.general.default_provider;
        let model_name = &self.config.general.default_model;
        let model_key = format!("{provider_name}/{model_name}");
        let caps = capabilities::get_capabilities(&model_key);
        let overrides = ProfileOverrides::from_config(&self.config.conversation);
        let profile = compaction::derive_profile(&caps, provider_name, &overrides);

        if profile.pipeline.is_empty()
            || profile.pipeline == vec![compaction::CompactionStrategy::None]
        {
            self.status_bar
                .set_status("Compaction disabled".to_string());
            return;
        }

        if conv.messages.len() <= profile.recent_messages {
            self.status_bar
                .set_status("Conversation too short to compact".to_string());
            return;
        }

        // Save a checkpoint before compacting
        let max_checkpoints = self.config.conversation.max_checkpoints;
        if let Some(store) = &self.conversations.store {
            if let Err(e) =
                store.save_checkpoint(conv.id, &conv.messages, Some("pre-compaction"))
            {
                tracing::warn!("Failed to save checkpoint: {e}");
            }
            let _ = store.prune_checkpoints(conv.id, max_checkpoints);
        }

        let messages = conv.messages.clone();
        let pinned = conv.pinned_messages.clone();
        let registry = Arc::clone(&self.registry);
        let provider_name_owned = provider_name.clone();
        let model_name_owned = model_name.clone();

        let (tx, rx) = mpsc::unbounded_channel();
        self.compaction_rx = Some(rx);
        self.status_bar.set_status("Compacting...".to_string());

        tokio::spawn(async move {
            let provider_ref = registry.get(&provider_name_owned);
            let provider_arg: Option<(&dyn crate::llm::LlmProvider, &str)> =
                provider_ref.map(|p| (p, model_name_owned.as_str()));

            let result = compaction::run_compaction_pipeline(
                &messages,
                &pinned,
                &profile,
                &caps,
                provider_arg,
                custom_instructions.as_deref(),
            )
            .await;

            match result {
                Ok(pipeline_result) => {
                    tx.send(pipeline_result).ok();
                }
                Err(e) => {
                    tracing::error!("Compaction pipeline failed: {e}");
                    // Send a minimal result with no changes so the UI can recover
                    tx.send(PipelineResult {
                        messages,
                        summary: None,
                        total_removed: 0,
                        total_tokens_reclaimed: 0,
                        steps_applied: vec![],
                    })
                    .ok();
                }
            }
        });
    }

    /// Check if auto-compaction should trigger after a streaming response.
    pub(crate) fn should_auto_compact(&self) -> bool {
        if self.config.conversation.compaction_strategy == "none" {
            return false;
        }
        if self.compaction_rx.is_some() {
            return false;
        }
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => return false,
        };

        let provider_name = &self.config.general.default_provider;
        let model_name = &self.config.general.default_model;
        let model_key = format!("{provider_name}/{model_name}");
        let caps = capabilities::get_capabilities(&model_key);
        let overrides = ProfileOverrides::from_config(&self.config.conversation);
        let profile = compaction::derive_profile(&caps, provider_name, &overrides);

        if conv.messages.len() <= profile.recent_messages {
            return false;
        }

        // Estimate current usage fraction
        let est_tokens = capabilities::estimate_message_tokens(&conv.messages);
        let budget =
            (caps.context_window as f64 * self.config.conversation.budget_fraction) as u32;
        if budget == 0 {
            return false;
        }
        let usage_fraction = est_tokens as f64 / budget as f64;
        usage_fraction >= profile.trigger_threshold
    }

    /// Drain a completed compaction result and apply it to the conversation.
    pub(crate) fn drain_compaction_result(&mut self) {
        let rx = match self.compaction_rx.as_mut() {
            Some(rx) => rx,
            None => return,
        };

        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(_) => return,
        };
        self.compaction_rx = None;

        // Verify we still have the same conversation
        let conv = match &mut self.conversations.active_conversation {
            Some(c) => c,
            None => return,
        };

        let messages_before = conv.messages.len();

        if result.total_removed == 0 && result.steps_applied.is_empty() {
            self.status_bar
                .set_status("Compaction: no changes needed".to_string());
            return;
        }

        // Apply compaction result
        self.compaction_summary = result.summary.clone();
        conv.messages = result.messages;
        conv.updated_at = chrono::Utc::now();

        // Record compaction event
        let mode = result
            .steps_applied
            .last()
            .copied()
            .unwrap_or(crate::store::types::CompactionMode::Truncation);
        conv.compaction_history
            .push(crate::store::types::CompactionEvent {
                timestamp: chrono::Utc::now(),
                mode,
                summary_preview: result
                    .summary
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect(),
                messages_before,
                messages_dropped: result.total_removed,
                tokens_reclaimed: result.total_tokens_reclaimed,
                checkpoint_id: None,
            });

        self.save_active_conversation();

        // Refresh chat view
        self.refresh_chat_view();

        // Refresh status bar health and pulse data
        self.refresh_pulse_data();

        let steps: Vec<&str> = result
            .steps_applied
            .iter()
            .map(|s| match s {
                crate::store::types::CompactionMode::ToolClearing => "tool clearing",
                crate::store::types::CompactionMode::Truncation => "truncation",
                crate::store::types::CompactionMode::Summarization => "summarization",
                crate::store::types::CompactionMode::Server => "server",
            })
            .collect();

        self.status_bar.set_status(format!(
            "Compacted: removed {} messages ({})",
            result.total_removed,
            steps.join(" + "),
        ));
    }

    /// Refresh data shown in the Pulse overlay and status bar health indicator.
    pub(crate) fn refresh_pulse_data(&mut self) {
        use crate::llm::context::ContextBudget;
        use crate::llm::health::HealthState;
        use crate::ui::components::pulse_overlay::{PinnedPreview, PulseStats};

        let Some(conv) = &self.conversations.active_conversation else {
            return;
        };

        // Build a lightweight budget from the last known context estimate
        let usage_fraction = if conv.context_estimate > 0 {
            let model = format!(
                "{}/{}",
                self.config.general.default_provider, self.config.general.default_model
            );
            let caps = capabilities::get_capabilities(&model);
            let budget =
                (caps.context_window as f64 * self.config.conversation.budget_fraction) as u32;
            if budget > 0 {
                conv.context_estimate as f32 / budget as f32
            } else {
                0.0
            }
        } else {
            0.0
        };

        let budget = ContextBudget {
            usage_fraction,
            health: HealthState::from_usage(usage_fraction),
            has_compaction_summary: self.compaction_summary.is_some(),
            ..Default::default()
        };

        let turn_count = conv.messages.len() as u32 / 2; // rough estimate
        let stats = PulseStats {
            turn_count,
            total_input_tokens: self.session_usage.input_tokens,
            total_output_tokens: self.session_usage.output_tokens,
            total_cost: self.session_usage.cost,
            cache_hit_rate: if self.session_usage.input_tokens > 0 {
                Some(
                    self.session_usage.cache_read_tokens as f32
                        / self.session_usage.input_tokens as f32,
                )
            } else {
                None
            },
            est_cost_per_message: if turn_count > 0 {
                self.session_usage.cost / turn_count as f64
            } else {
                0.0
            },
            compaction_count: conv.compaction_history.len(),
            dropped_message_count: conv
                .compaction_history
                .iter()
                .map(|e| e.messages_dropped)
                .sum(),
        };

        let pinned = conv
            .pinned_messages
            .iter()
            .filter_map(|&idx| {
                conv.messages.get(idx).map(|m| PinnedPreview {
                    message_index: idx,
                    role: m.role.as_str().to_string(),
                    preview: m.content.chars().take(60).collect(),
                })
            })
            .collect();

        let health = budget.health;
        let has_summary = budget.has_compaction_summary;

        self.pulse_overlay.refresh(
            budget,
            stats,
            pinned,
            conv.session_notes.clone().unwrap_or_default(),
            conv.compaction_history.clone(),
            self.config.conversation.compaction_mode.clone(),
        );

        // Update status bar health
        self.status_bar.health_state = Some(health);
        self.status_bar.usage_pct = Some((usage_fraction * 100.0) as u32);
        self.status_bar.has_compaction_summary = has_summary;
    }

    /// Refresh the chat view from the active conversation's messages.
    pub(crate) fn refresh_chat_view(&mut self) {
        self.chat_view.clear();
        if let Some(conv) = &self.conversations.active_conversation {
            for msg in &conv.messages {
                let role = match msg.role {
                    crate::llm::types::Role::User => MessageRole::User,
                    crate::llm::types::Role::Assistant => MessageRole::Assistant,
                    _ => MessageRole::System,
                };
                self.chat_view.add_message(ChatMessage {
                    role,
                    content: msg.content.clone(),
                    timestamp: None,
                });
            }
        }
    }
}
