/// Streaming lifecycle methods for [`App`].
///
/// Handles starting an LLM streaming request, draining incoming chunks
/// from the channel, and finalising the response once complete.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::app::App;
use crate::llm::compaction::{self, ProfileOverrides};
use crate::llm::types::{ChatRequest, Message, StreamChunk, ToolCall, TokenUsage};
use crate::llm::{capabilities, context, pricing};
use crate::ui::components::chat_view::{ChatMessage, MessageRole};

impl App {
    /// Spawn a background task to stream an LLM response.
    ///
    /// When MCP tools are available, runs a tool-call loop:
    /// call LLM → execute tools → call LLM again → repeat until done.
    pub(crate) fn start_streaming(&mut self) {
        let provider_name = &self.config.general.default_provider;
        let _provider = match self.registry.get(provider_name) {
            Some(p) => p,
            None => {
                self.status_bar
                    .set_status(format!("No provider: {provider_name}"));
                return;
            }
        };

        // Resolve model: per-provider default_model > general.default_model
        let model = self
            .config
            .providers
            .get(provider_name)
            .and_then(|p| p.default_model.clone())
            .unwrap_or_else(|| self.config.general.default_model.clone());

        // Build system and context messages
        let mut system_messages = Vec::new();
        if let Some(ref prompt) = self.config.general.system_prompt {
            system_messages.push(Message::system(prompt.clone()));
        }

        let context_messages: Vec<Message> = self
            .active_context
            .as_ref()
            .and_then(|name| self.contexts.get(name))
            .map(|ctx| ctx.build_messages())
            .unwrap_or_default();

        // Attach MCP tools to the request if available
        let tool_defs = self
            .mcp_manager
            .as_ref()
            .filter(|m| m.has_tools())
            .map(|m| m.tool_definitions())
            .unwrap_or_default();

        // Budget-aware context assembly using profile-derived settings
        let caps = capabilities::get_capabilities(&model);
        let overrides = ProfileOverrides::from_config(&self.config.conversation);
        let profile = compaction::derive_profile(&caps, provider_name, &overrides);
        let ctx_config = context::ContextConfig {
            recent_message_count: profile.recent_messages,
            budget_fraction: self.config.conversation.budget_fraction,
            ..context::ContextConfig::default()
        };
        let pinned = self
            .conversations
            .active_conversation
            .as_ref()
            .map(|c| c.pinned_messages.as_slice())
            .unwrap_or(&[]);
        let assembled = context::assemble_context(
            &system_messages,
            &context_messages,
            self.messages(),
            self.compaction_summary.as_deref(),
            tool_defs.len(),
            &caps,
            &ctx_config,
            pinned,
        );

        // Update context estimate on the conversation
        if let Some(conv) = &mut self.conversations.active_conversation {
            conv.context_estimate = assembled.estimated_tokens;
        }

        // Update context usage display
        if self.config.usage.show_context_usage {
            let pct = (assembled.usage_fraction * 100.0) as u32;
            self.model_selector.context_usage_pct = Some(pct);
            if assembled.compaction_recommended {
                tracing::info!("Context usage: {pct}% — compaction recommended");
            }
        }

        let mut request = ChatRequest::new(model.clone(), assembled.messages).with_tools(tool_defs);
        if let Some(temp) = self.config.general.temperature {
            request = request.with_temperature(temp);
        }
        if let Some(max) = self.config.general.max_tokens {
            request = request.with_max_tokens(max);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        self.stream_rx = Some(rx);
        self.streaming = true;
        // Seed usage with model/provider info for cost calculation later
        self.last_usage = Some(TokenUsage {
            model: Some(model.clone()),
            provider: Some(provider_name.clone()),
            ..Default::default()
        });

        // Add empty assistant message that we'll append chunks to
        self.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: Some(chrono::Local::now()),
        });

        self.status_bar.set_status("streaming...".to_string());

        let registry = Arc::clone(&self.registry);
        let provider_name = provider_name.clone();
        let mcp_manager = self.mcp_manager.clone();

        tokio::spawn(async move {
            let Some(provider) = registry.get(&provider_name) else {
                tx.send(StreamChunk::Error(format!("No provider: {provider_name}")))
                    .ok();
                return;
            };

            // Get the tool definitions for re-use in the loop
            let tool_defs = mcp_manager
                .as_ref()
                .filter(|m| m.has_tools())
                .map(|m| m.tool_definitions())
                .unwrap_or_default();
            let has_mcp = !tool_defs.is_empty();

            // First call
            let mut current_request = request;

            loop {
                // Create a per-iteration channel to collect this round's chunks
                let (iter_tx, mut iter_rx) = mpsc::unbounded_channel();

                if let Err(e) = provider.chat(current_request.clone(), iter_tx).await {
                    tx.send(StreamChunk::Error(e.to_string())).ok();
                    return;
                }

                // Collect all chunks, forwarding text/usage to the main channel
                // and accumulating tool calls
                let mut tool_calls: Vec<ToolCall> = Vec::new();
                let mut got_done = false;

                while let Some(chunk) = iter_rx.recv().await {
                    match chunk {
                        StreamChunk::ToolCallStart {
                            id,
                            name,
                            arguments,
                        } => {
                            // Forward to UI for display
                            tx.send(StreamChunk::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            })
                            .ok();
                            tool_calls.push(ToolCall { id, name, arguments });
                        }
                        StreamChunk::Done => {
                            got_done = true;
                            break;
                        }
                        other => {
                            if tx.send(other).is_err() {
                                return;
                            }
                        }
                    }
                }

                // If we got tool calls and have MCP, execute them and loop
                if !tool_calls.is_empty() && has_mcp {
                    let mcp = mcp_manager.as_ref().unwrap();

                    // Add assistant tool_use message to conversation
                    current_request
                        .messages
                        .push(Message::tool_use(tool_calls.clone()));

                    for tc in &tool_calls {
                        let arguments: serde_json::Value =
                            serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

                        let result = match mcp.call_tool(&tc.name, arguments).await {
                            Ok(r) => r,
                            Err(e) => crate::mcp::types::ToolCallResult {
                                content: format!("Error: {e}"),
                                is_error: true,
                            },
                        };

                        // Send result to UI
                        tx.send(StreamChunk::ToolCallResult {
                            id: tc.id.clone(),
                            content: result.content.clone(),
                            is_error: result.is_error,
                        })
                        .ok();

                        // Add tool result message to conversation
                        current_request
                            .messages
                            .push(Message::tool_result(&tc.id, &result.content));
                    }

                    // Continue the loop — call LLM again with tool results
                    continue;
                }

                // No tool calls or no MCP — we're done
                if got_done {
                    tx.send(StreamChunk::Done).ok();
                }
                break;
            }
        });
    }

    /// Drain any pending stream chunks from the receiver.
    pub(crate) fn drain_stream_chunks(&mut self) {
        let rx = match self.stream_rx.as_mut() {
            Some(rx) => rx,
            None => return,
        };

        loop {
            match rx.try_recv() {
                Ok(StreamChunk::Delta(text)) => {
                    self.chat_view.append_to_last(&text);
                }
                Ok(StreamChunk::Usage(usage)) => {
                    // Accumulate partial usage (Anthropic sends input + output separately)
                    let current = self.last_usage.get_or_insert(TokenUsage::default());
                    current.accumulate(&usage);
                }
                Ok(StreamChunk::ToolCallStart { name, .. }) => {
                    self.chat_view
                        .append_to_last(&format!("\n[Calling tool: {name}...]"));
                    self.status_bar
                        .set_status(format!("calling tool: {name}..."));
                }
                Ok(StreamChunk::ToolCallResult {
                    content, is_error, ..
                }) => {
                    let prefix = if is_error { "Tool error" } else { "Tool result" };
                    // Show a truncated preview of the result inline
                    let preview = if content.len() > 200 {
                        format!("{}...", &content[..200])
                    } else {
                        content
                    };
                    self.chat_view
                        .append_to_last(&format!("\n[{prefix}: {preview}]"));
                    self.status_bar.set_status("streaming...".to_string());
                }
                Ok(StreamChunk::CompactionOccurred {
                    summary_preview,
                    messages_before,
                }) => {
                    tracing::info!("Server compacted: {messages_before} messages → summary");
                    self.status_bar.set_status(format!(
                        "Server compacted: {messages_before} msgs → summary"
                    ));
                    // Store as compaction summary for context assembly
                    self.compaction_summary = Some(summary_preview);
                }
                Ok(StreamChunk::Done) => {
                    self.finish_streaming();
                    break;
                }
                Ok(StreamChunk::Error(msg)) => {
                    self.chat_view
                        .append_to_last(&format!("\n[Error: {msg}]"));
                    self.finish_streaming();
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.finish_streaming();
                    break;
                }
            }
        }
    }

    /// Finalise a streaming response: save the message, calculate costs,
    /// update usage stats, and trigger auto-compaction if needed.
    pub(crate) fn finish_streaming(&mut self) {
        self.streaming = false;
        self.stream_rx = None;

        // Save the assistant response to the conversation
        if let Some(last) = self.chat_view.messages.last()
            && last.role == MessageRole::Assistant
        {
            self.conversations
                .add_message(Message::assistant(&last.content));
        }

        // Calculate cost from pricing registry (or custom overrides) before consuming usage
        if let Some(usage) = self.last_usage.as_mut() {
            let model = usage
                .model
                .as_deref()
                .unwrap_or(&self.config.general.default_model);

            // Check custom pricing first, then built-in
            let model_pricing = self
                .config
                .usage
                .custom_pricing
                .get(model)
                .map(|cp| {
                    pricing::ModelPricing::new(cp.input_per_million, cp.output_per_million)
                        .with_cache(cp.cache_read_per_million, cp.cache_write_per_million)
                })
                .or_else(|| pricing::get_pricing(model));

            if let Some(mp) = model_pricing {
                usage.cost = mp.calculate_cost(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens,
                );
            }

            // Cost warning
            if let Some(threshold) = self.config.usage.cost_warning_threshold {
                if usage.cost > threshold {
                    tracing::warn!(
                        "Turn cost ${:.4} exceeds warning threshold ${:.4}",
                        usage.cost,
                        threshold
                    );
                }
            }
        }

        // Update conversation-level usage totals
        if let Some(usage) = &self.last_usage {
            if let Some(conv) = &mut self.conversations.active_conversation {
                conv.total_input_tokens += usage.input_tokens;
                conv.total_output_tokens += usage.output_tokens;
                conv.total_cache_tokens += usage.cache_read_tokens + usage.cache_creation_tokens;
                conv.total_cost += usage.cost;
                conv.turn_count += 1;
            }
        }

        self.save_active_conversation();

        // Build status with token usage if available
        let status = if let Some(usage) = self.last_usage.take() {
            self.session_usage.accumulate(&usage);
            let mut parts = vec!["ready".to_string()];

            if self.config.usage.show_token_usage {
                parts.push(format!(
                    "{}in + {}out",
                    usage.input_tokens, usage.output_tokens
                ));
            }

            if self.config.usage.show_cost && usage.cost > 0.0 {
                parts.push(pricing::format_cost(usage.cost));
            }

            if self.config.usage.show_token_usage && self.session_usage.total() > 0 {
                let session_str = if self.config.usage.show_cost && self.session_usage.cost > 0.0 {
                    format!(
                        "session: {} ({})",
                        self.session_usage.total(),
                        pricing::format_cost(self.session_usage.cost)
                    )
                } else {
                    format!("session: {}", self.session_usage.total())
                };
                parts.push(session_str);
            }

            parts.join(" | ")
        } else {
            "ready".to_string()
        };
        self.status_bar.set_status(status);

        // Check if auto-compaction should trigger
        if self.should_auto_compact() {
            self.spawn_compaction(None);
        }
    }
}
