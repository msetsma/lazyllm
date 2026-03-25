/// Command execution methods for [`App`].
///
/// Handles `:command` mode dispatch, runtime config changes,
/// usage/cost reporting, export/import, and clipboard operations.

use crate::app::App;
use crate::command::{self, Command};
use crate::event::types::Action;
use crate::llm::pricing;
use crate::llm::types::Message;
use crate::ui::components::chat_list::ChatList;
use crate::ui::components::chat_view::{ChatMessage, MessageRole};
use crate::ui::components::Component;

impl App {
    /// Execute a parsed command from command mode.
    pub(crate) fn execute_command(&mut self, input: &str) {
        match command::parse_command(input) {
            Command::Quit => {
                self.save_active_conversation();
                self.running = false;
            }
            Command::Model(name) => {
                if let Some(provider_name) = self.find_model(&name) {
                    self.set_active_model(provider_name, name);
                } else {
                    self.status_bar
                        .set_status(format!("Unknown model: {name}"));
                }
            }
            Command::Provider(name) => {
                if self.registry.get(&name).is_some() {
                    let model = self.config.general.default_model.clone();
                    self.set_active_model(name, model);
                } else {
                    self.status_bar
                        .set_status(format!("Unknown provider: {name}"));
                }
            }
            Command::NewChat => {
                self.create_new_conversation();
            }
            Command::DeleteChat => {
                self.delete_selected_conversation();
            }
            Command::Help => {
                self.help_overlay.handle_action(&Action::ToggleHelp);
            }
            Command::Clear => {
                self.chat_view.clear();
                if let Some(conv) = &mut self.conversations.active_conversation {
                    conv.messages.clear();
                }
                self.save_active_conversation();
            }
            Command::Context(Some(name)) => {
                if !self.config.features.contexts {
                    self.status_bar
                        .set_status("Contexts feature is disabled in config".to_string());
                } else if self.contexts.contains_key(&name) {
                    self.active_context = Some(name.clone());
                    self.model_selector.context_name = Some(name.clone());
                    if let Some(conv) = &mut self.conversations.active_conversation {
                        conv.context_name = Some(name.clone());
                    }
                    self.status_bar
                        .set_status(format!("Context: {name}"));
                } else {
                    let available: Vec<_> = self.contexts.keys().cloned().collect();
                    if available.is_empty() {
                        self.status_bar.set_status(
                            "No contexts found. Add .toml files to contexts dir.".to_string(),
                        );
                    } else {
                        self.status_bar.set_status(format!(
                            "Unknown context: {name}. Available: {}",
                            available.join(", ")
                        ));
                    }
                }
            }
            Command::Context(None) => {
                if !self.config.features.contexts {
                    self.status_bar
                        .set_status("Contexts feature is disabled in config".to_string());
                } else {
                    self.active_context = None;
                    self.model_selector.context_name = None;
                    if let Some(conv) = &mut self.conversations.active_conversation {
                        conv.context_name = None;
                    }
                    self.status_bar.set_status("Context cleared".to_string());
                }
            }
            Command::Export => {
                self.export_active_conversation();
            }
            Command::Import(path) => {
                self.import_conversation(&path);
            }
            Command::Usage => {
                self.show_usage();
            }
            Command::Spend => {
                self.show_spend();
            }
            Command::Compact(custom) => {
                self.spawn_compaction(custom);
            }
            Command::Checkpoints => {
                self.show_checkpoints();
            }
            Command::Restore(id) => {
                self.restore_checkpoint(id);
            }
            Command::Set(key, value) => {
                self.set_config(&key, &value);
            }
            Command::Pulse => {
                self.pulse_overlay.visible = !self.pulse_overlay.visible;
                if self.pulse_overlay.visible {
                    self.refresh_pulse_data();
                }
            }
            Command::EditSessionNotes => {
                let notes = self
                    .conversations
                    .active_conversation
                    .as_ref()
                    .and_then(|c| c.session_notes.as_deref())
                    .unwrap_or("");
                self.notes_editor.open(notes);
            }
            Command::TogglePin => {
                if let Some(conv) = &mut self.conversations.active_conversation {
                    let idx = self.chat_view.selected_message_index();
                    if let Some(pos) = conv.pinned_messages.iter().position(|&i| i == idx) {
                        conv.pinned_messages.remove(pos);
                        self.status_bar
                            .set_status(format!("Unpinned message #{idx}"));
                    } else {
                        conv.pinned_messages.push(idx);
                        self.status_bar
                            .set_status(format!("Pinned message #{idx}"));
                    }
                }
                self.save_active_conversation();
            }
            Command::Unknown(cmd) => {
                self.status_bar
                    .set_status(format!("Unknown command: {cmd}"));
            }
        }
    }

    /// Export the active conversation to a JSON file in the data directory.
    pub(crate) fn export_active_conversation(&mut self) {
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => {
                self.status_bar
                    .set_status("No active conversation to export".to_string());
                return;
            }
        };

        let store = match &self.conversations.store {
            Some(s) => s,
            None => {
                self.status_bar
                    .set_status("No store available".to_string());
                return;
            }
        };

        match store.export_conversation(conv.id) {
            Ok(json) => {
                let export_dir = self.config.general.data_dir.join("exports");
                if let Err(e) = std::fs::create_dir_all(&export_dir) {
                    self.status_bar
                        .set_status(format!("Failed to create export dir: {e}"));
                    return;
                }
                let path = export_dir.join(format!("{}.json", conv.id));
                match std::fs::write(&path, json) {
                    Ok(()) => {
                        self.status_bar
                            .set_status(format!("Exported to {}", path.display()));
                    }
                    Err(e) => {
                        self.status_bar
                            .set_status(format!("Export failed: {e}"));
                    }
                }
            }
            Err(e) => {
                self.status_bar
                    .set_status(format!("Export failed: {e}"));
            }
        }
    }

    /// Import a conversation from a JSON file.
    pub(crate) fn import_conversation(&mut self, path: &str) {
        let store = match &self.conversations.store {
            Some(s) => s,
            None => {
                self.status_bar
                    .set_status("No store available".to_string());
                return;
            }
        };

        let json = match std::fs::read_to_string(path) {
            Ok(j) => j,
            Err(e) => {
                self.status_bar
                    .set_status(format!("Failed to read file: {e}"));
                return;
            }
        };

        match store.import_conversation(&json) {
            Ok(id) => {
                self.conversations.conversation_ids.insert(0, id);
                // Reload the conversation list to get the title
                if let Some(list) = self.conversations.load_conversation_list() {
                    self.chat_list = ChatList::from_items(list.titles);
                }
                self.status_bar
                    .set_status(format!("Imported conversation {id}"));
            }
            Err(e) => {
                self.status_bar
                    .set_status(format!("Import failed: {e}"));
            }
        }
    }

    /// Show token usage stats for the current conversation.
    pub(crate) fn show_usage(&mut self) {
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => {
                self.status_bar
                    .set_status("No active conversation".to_string());
                return;
            }
        };

        let total_tokens = conv.total_input_tokens + conv.total_output_tokens;
        let mut info = format!(
            "Usage: {}in + {}out = {} tokens | {} turns",
            conv.total_input_tokens, conv.total_output_tokens, total_tokens, conv.turn_count
        );
        if conv.total_cache_tokens > 0 {
            info.push_str(&format!(" | cache: {}", conv.total_cache_tokens));
        }
        if conv.total_cost > 0.0 {
            info.push_str(&format!(" | cost: {}", pricing::format_cost(conv.total_cost)));
        }
        // Add session usage
        if self.session_usage.total() > 0 {
            info.push_str(&format!(
                " | session: {} ({})",
                self.session_usage.total(),
                pricing::format_cost(self.session_usage.cost)
            ));
        }
        self.chat_view.add_message(ChatMessage {
            role: MessageRole::System,
            content: info,
            timestamp: Some(chrono::Local::now()),
        });
    }

    /// Show cost report across all conversations.
    pub(crate) fn show_spend(&mut self) {
        let store = match &self.conversations.store {
            Some(s) => s,
            None => {
                self.status_bar
                    .set_status("No store available".to_string());
                return;
            }
        };

        let summaries = match store.list() {
            Ok(s) => s,
            Err(e) => {
                self.status_bar
                    .set_status(format!("Failed to load conversations: {e}"));
                return;
            }
        };

        let mut total_cost = 0.0;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut total_turns = 0u32;
        let mut lines = vec!["Cost Report:".to_string()];

        for s in &summaries {
            total_cost += s.total_cost;
            total_input += s.total_input_tokens as u64;
            total_output += s.total_output_tokens as u64;
            total_turns += s.turn_count;
            if s.total_cost > 0.0 {
                lines.push(format!(
                    "  {} | {} | {} turns | {}",
                    truncate_title(&s.title, 30),
                    s.model,
                    s.turn_count,
                    pricing::format_cost(s.total_cost),
                ));
            }
        }

        lines.push(format!(
            "Total: {}in + {}out = {} tokens | {} turns | {}",
            total_input,
            total_output,
            total_input + total_output,
            total_turns,
            pricing::format_cost(total_cost)
        ));

        // Add session totals
        if self.session_usage.total() > 0 {
            lines.push(format!(
                "Session: {} tokens | {}",
                self.session_usage.total(),
                pricing::format_cost(self.session_usage.cost)
            ));
        }

        self.chat_view.add_message(ChatMessage {
            role: MessageRole::System,
            content: lines.join("\n"),
            timestamp: Some(chrono::Local::now()),
        });
    }

    /// Set a runtime configuration value.
    pub(crate) fn set_config(&mut self, key: &str, value: &str) {
        match key {
            "temperature" => match value.parse::<f32>() {
                Ok(t) if (0.0..=2.0).contains(&t) => {
                    self.config.general.temperature = Some(t);
                    self.status_bar
                        .set_status(format!("temperature = {t}"));
                }
                _ => {
                    self.status_bar
                        .set_status("temperature must be 0.0–2.0".to_string());
                }
            },
            "max_tokens" => match value.parse::<u32>() {
                Ok(n) if n > 0 => {
                    self.config.general.max_tokens = Some(n);
                    self.status_bar
                        .set_status(format!("max_tokens = {n}"));
                }
                _ => {
                    self.status_bar
                        .set_status("max_tokens must be a positive integer".to_string());
                }
            },
            "compaction" | "compaction_strategy" => {
                let valid = ["auto", "none", "truncation", "summarization"];
                if valid.contains(&value) {
                    self.config.conversation.compaction_strategy = value.to_string();
                    self.status_bar
                        .set_status(format!("compaction_strategy = {value}"));
                } else {
                    self.status_bar.set_status(format!(
                        "Invalid strategy. Use: {}",
                        valid.join(", ")
                    ));
                }
            }
            "recent_messages" => match value.parse::<usize>() {
                Ok(n) if n > 0 => {
                    self.config.conversation.recent_messages = n;
                    self.status_bar
                        .set_status(format!("recent_messages = {n}"));
                }
                _ => {
                    self.status_bar
                        .set_status("recent_messages must be a positive integer".to_string());
                }
            },
            "show_cost" => match value {
                "true" | "on" | "1" => {
                    self.config.usage.show_cost = true;
                    self.status_bar.set_status("show_cost = true".to_string());
                }
                "false" | "off" | "0" => {
                    self.config.usage.show_cost = false;
                    self.status_bar.set_status("show_cost = false".to_string());
                }
                _ => {
                    self.status_bar
                        .set_status("show_cost must be true/false".to_string());
                }
            },
            "show_tokens" | "show_token_usage" => match value {
                "true" | "on" | "1" => {
                    self.config.usage.show_token_usage = true;
                    self.status_bar
                        .set_status("show_token_usage = true".to_string());
                }
                "false" | "off" | "0" => {
                    self.config.usage.show_token_usage = false;
                    self.status_bar
                        .set_status("show_token_usage = false".to_string());
                }
                _ => {
                    self.status_bar
                        .set_status("show_token_usage must be true/false".to_string());
                }
            },
            "timestamps" | "show_timestamps" => match value {
                "true" | "on" | "1" => {
                    self.config.ui.show_timestamps = true;
                    self.chat_view.set_show_timestamps(true);
                    self.status_bar
                        .set_status("show_timestamps = true".to_string());
                }
                "false" | "off" | "0" => {
                    self.config.ui.show_timestamps = false;
                    self.chat_view.set_show_timestamps(false);
                    self.status_bar
                        .set_status("show_timestamps = false".to_string());
                }
                _ => {
                    self.status_bar
                        .set_status("show_timestamps must be true/false".to_string());
                }
            },
            "system_prompt" => {
                if value == "none" || value == "clear" {
                    self.config.general.system_prompt = None;
                    self.status_bar
                        .set_status("system_prompt cleared".to_string());
                } else {
                    self.config.general.system_prompt = Some(value.to_string());
                    self.status_bar
                        .set_status("system_prompt set".to_string());
                }
            }
            _ => {
                self.status_bar
                    .set_status(format!("Unknown setting: {key}"));
            }
        }
    }

    /// Show checkpoints for the active conversation.
    pub(crate) fn show_checkpoints(&mut self) {
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => {
                self.status_bar
                    .set_status("No active conversation".to_string());
                return;
            }
        };

        let store = match &self.conversations.store {
            Some(s) => s,
            None => {
                self.status_bar
                    .set_status("No store available".to_string());
                return;
            }
        };

        match store.load_checkpoints(conv.id) {
            Ok(checkpoints) => {
                if checkpoints.is_empty() {
                    self.chat_view.add_message(ChatMessage {
                        role: MessageRole::System,
                        content: "No checkpoints saved for this conversation.".to_string(),
                        timestamp: Some(chrono::Local::now()),
                    });
                } else {
                    let mut lines = vec![format!("Checkpoints ({}):", checkpoints.len())];
                    for cp in &checkpoints {
                        let reason = cp.reason.as_deref().unwrap_or("manual");
                        lines.push(format!(
                            "  #{} | {} | {}",
                            cp.id,
                            cp.created_at.format("%Y-%m-%d %H:%M"),
                            reason
                        ));
                    }
                    lines.push("Use :restore <id> to restore a checkpoint.".to_string());
                    self.chat_view.add_message(ChatMessage {
                        role: MessageRole::System,
                        content: lines.join("\n"),
                        timestamp: Some(chrono::Local::now()),
                    });
                }
            }
            Err(e) => {
                self.status_bar
                    .set_status(format!("Failed to load checkpoints: {e}"));
            }
        }
    }

    /// Restore a checkpoint by ID (or latest if None).
    pub(crate) fn restore_checkpoint(&mut self, checkpoint_id: Option<i64>) {
        let conv = match &self.conversations.active_conversation {
            Some(c) => c,
            None => {
                self.status_bar
                    .set_status("No active conversation".to_string());
                return;
            }
        };

        let store = match &self.conversations.store {
            Some(s) => s,
            None => {
                self.status_bar
                    .set_status("No store available".to_string());
                return;
            }
        };

        let checkpoints = match store.load_checkpoints(conv.id) {
            Ok(cps) => cps,
            Err(e) => {
                self.status_bar
                    .set_status(format!("Failed to load checkpoints: {e}"));
                return;
            }
        };

        if checkpoints.is_empty() {
            self.status_bar
                .set_status("No checkpoints available".to_string());
            return;
        }

        let checkpoint = match checkpoint_id {
            Some(id) => checkpoints.iter().find(|c| c.id == id),
            None => checkpoints.last(),
        };

        let checkpoint = match checkpoint {
            Some(cp) => cp,
            None => {
                self.status_bar
                    .set_status("Checkpoint not found".to_string());
                return;
            }
        };

        // Parse the snapshot
        let messages: Vec<Message> = match serde_json::from_str(&checkpoint.snapshot_json) {
            Ok(msgs) => msgs,
            Err(e) => {
                self.status_bar
                    .set_status(format!("Failed to parse checkpoint: {e}"));
                return;
            }
        };

        let cp_id = checkpoint.id;

        // Restore messages
        if let Some(conv) = &mut self.conversations.active_conversation {
            conv.messages = messages;
            conv.updated_at = chrono::Utc::now();
            // Remove the most recent compaction history entry (the one we're undoing)
            conv.compaction_history.pop();
        }
        self.compaction_summary = None;
        self.save_active_conversation();

        // Delete the consumed checkpoint
        if let Some(store) = &self.conversations.store {
            let _ = store.delete_checkpoint(cp_id);
        }

        // Refresh chat view and status bar health
        self.refresh_chat_view();
        self.refresh_pulse_data();

        self.status_bar
            .set_status(format!("Restored checkpoint #{cp_id}"));
    }

    /// Copy the selected (visual mode) or last assistant response to clipboard.
    pub(crate) fn copy_last_response(&mut self) {
        // In visual mode, copy whichever message is selected
        let content = if let Some(text) = self.chat_view.selected_content() {
            if text.is_empty() {
                self.status_bar
                    .set_status("Nothing to copy".to_string());
                return;
            }
            text.to_string()
        } else {
            // Fallback: last assistant message
            match self
                .chat_view
                .messages
                .iter()
                .rev()
                .find(|m| m.role == MessageRole::Assistant)
            {
                Some(msg) if !msg.content.is_empty() => msg.content.clone(),
                Some(_) => {
                    self.status_bar
                        .set_status("Nothing to copy".to_string());
                    return;
                }
                None => {
                    self.status_bar
                        .set_status("No assistant message to copy".to_string());
                    return;
                }
            }
        };

        match crate::app::copy_to_clipboard(&content) {
            Ok(()) => {
                let preview = if content.len() > 60 {
                    format!("{}...", &content[..60])
                } else {
                    content
                };
                self.status_bar
                    .set_status(format!("Copied: {preview}"));
            }
            Err(e) => {
                self.status_bar
                    .set_status(format!("Copy failed: {e}"));
            }
        }
    }
}

fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        format!("{}...", &title[..max_len.saturating_sub(3)])
    }
}
