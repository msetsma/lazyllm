use std::io::Write;
use std::sync::Arc;

use base64::Engine;
use tokio::sync::mpsc;

use crate::config::types::AppConfig;
use crate::context::Context;
use crate::conversation::ConversationManager;
use crate::event::types::{Action, FocusTarget, Mode};
use crate::llm::ProviderRegistry;
use crate::llm::types::{ChatRequest, Message, StreamChunk, ToolCall, TokenUsage};
use crate::mcp::McpManager;
use crate::store::json_store::JsonStore;
use crate::ui::components::chat_list::ChatList;
use crate::ui::components::chat_view::{ChatMessage, ChatView, MessageRole};
use crate::command::{self, Command};
use crate::ui::components::help_overlay::HelpOverlay;
use crate::ui::components::input_box::InputBox;
use crate::ui::components::model_popup::ModelPopup;
use crate::ui::components::model_selector::ModelSelector;
use crate::ui::components::status_bar::StatusBar;
use crate::ui::components::tool_panel::ToolPanel;
use crate::ui::components::Component;
use crate::ui::theme::{Theme, load_theme};

/// Central application state.
pub struct App {
    pub(crate) mode: Mode,
    pub(crate) focus: FocusTarget,
    pub(crate) running: bool,
    pub(crate) streaming: bool,
    pub(crate) config: AppConfig,
    pub(crate) theme: Theme,

    // LLM
    pub(crate) registry: Arc<ProviderRegistry>,
    pub(crate) stream_rx: Option<mpsc::UnboundedReceiver<StreamChunk>>,

    // MCP
    pub(crate) mcp_manager: Option<Arc<McpManager>>,

    // Contexts
    pub(crate) contexts: std::collections::HashMap<String, Context>,
    pub(crate) active_context: Option<String>,

    // Token usage
    pub(crate) last_usage: Option<TokenUsage>,
    pub(crate) session_usage: TokenUsage,

    // Persistence
    pub(crate) conversations: ConversationManager,

    // Components
    pub(crate) chat_list: ChatList,
    pub(crate) chat_view: ChatView,
    pub(crate) input_box: InputBox,
    pub(crate) model_selector: ModelSelector,
    pub(crate) status_bar: StatusBar,
    pub(crate) tool_panel: ToolPanel,
    pub(crate) help_overlay: HelpOverlay,
    pub(crate) model_popup: ModelPopup,
}

impl App {
    /// Read-only access to the message history for the active conversation.
    pub fn messages(&self) -> &[Message] {
        self.conversations.messages()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn focus(&self) -> FocusTarget {
        self.focus
    }
}

impl App {
    pub fn new(config: AppConfig, registry: ProviderRegistry) -> Self {
        let model_selector = ModelSelector::new(
            config.general.default_provider.clone(),
            config.general.default_model.clone(),
        );

        let theme = load_theme(&config.ui.theme);

        let mut chat_view = ChatView::new();
        chat_view.set_show_timestamps(config.ui.show_timestamps);
        chat_view.set_render_options(crate::markdown::RenderOptions {
            latex: config.features.latex_rendering,
            tables: config.features.table_rendering,
        });
        chat_view.set_markdown_rendering(config.ui.markdown_rendering);

        // Load contexts from disk (if enabled)
        let contexts = if config.features.contexts {
            crate::context::load_contexts(&config.general.contexts_dir)
        } else {
            std::collections::HashMap::new()
        };
        let active_context = config
            .general
            .default_context
            .clone()
            .filter(|name| contexts.contains_key(name));

        if !contexts.is_empty() {
            tracing::info!("Loaded {} context(s)", contexts.len());
        }

        let mut ms = model_selector;
        ms.context_name = active_context.clone();

        Self {
            mode: Mode::Normal,
            focus: FocusTarget::default(),
            running: true,
            streaming: false,
            theme,
            config,
            registry: Arc::new(registry),
            stream_rx: None,
            mcp_manager: None,
            contexts,
            active_context,
            last_usage: None,
            session_usage: TokenUsage::default(),
            conversations: ConversationManager::new(),
            chat_list: ChatList::new(),
            chat_view,
            input_box: InputBox::new(),
            model_selector: ms,
            status_bar: StatusBar::new(),
            tool_panel: ToolPanel::new(),
            help_overlay: HelpOverlay::new(),
            model_popup: ModelPopup::new(),
        }
    }

    /// Initialize with a store, loading existing conversations.
    pub fn with_store(mut self, store: JsonStore) -> Self {
        self.conversations = ConversationManager::new().with_store(store);
        if let Some(list) = self.conversations.load_conversation_list() {
            self.chat_list = ChatList::from_items(list.titles);
        }
        self
    }

    /// Initialize MCP servers from config.
    pub async fn init_mcp(&mut self) {
        if !self.config.features.mcp_servers {
            tracing::info!("MCP servers disabled in config");
            return;
        }

        let servers = &self.config.mcp.servers;
        if servers.is_empty() {
            tracing::info!("No MCP servers configured");
            return;
        }

        tracing::info!("Initializing {} MCP server(s)...", servers.len());
        let manager = McpManager::new(servers).await;

        // Populate UI components
        self.tool_panel.servers = manager.server_tools();
        self.model_selector = self.model_selector.with_mcp_count(manager.server_count());

        tracing::info!(
            "MCP initialized: {} server(s), {} tool(s)",
            manager.server_count(),
            manager.tool_definitions().len()
        );

        self.mcp_manager = Some(Arc::new(manager));
    }

    /// Shut down all MCP servers.
    pub async fn shutdown_mcp(&mut self) {
        if let Some(manager) = self.mcp_manager.take() {
            manager.shutdown_all().await;
        }
    }

    /// Switch to a conversation by index in the chat list.
    fn switch_to_conversation(&mut self, index: usize) {
        match self.conversations.switch_to_conversation(index) {
            Ok(conv) => {
                // Restore the model/provider from the conversation
                self.config.general.default_provider = conv.provider.clone();
                self.config.general.default_model = conv.model.clone();
                let mut ms = ModelSelector::new(conv.provider.clone(), conv.model.clone());

                // Restore context from conversation
                self.active_context = conv.context_name.clone();
                ms.context_name = conv.context_name.clone();
                self.model_selector = ms;

                let mut view = ChatView::from_messages(&conv.messages);
                view.set_show_timestamps(self.config.ui.show_timestamps);
                self.chat_view = view;
            }
            Err(msg) => {
                self.status_bar.set_status(msg);
            }
        }
    }

    /// Create a new conversation and switch to it.
    fn create_new_conversation(&mut self) {
        let provider = self.config.general.default_provider.clone();
        let model = self.config.general.default_model.clone();
        let (_id, title) = self.conversations.create_new_conversation(provider, model);

        // Apply active context to the new conversation
        if let Some(ref ctx_name) = self.active_context {
            if let Some(conv) = &mut self.conversations.active_conversation {
                conv.context_name = Some(ctx_name.clone());
            }
        }

        let mut view = ChatView::new();
        view.set_show_timestamps(self.config.ui.show_timestamps);
        self.chat_view = view;

        // Add to list and select it
        self.chat_list.items.insert(0, title);
        self.chat_list.state.select(Some(0));
    }

    /// Delete the currently selected conversation.
    fn delete_selected_conversation(&mut self) {
        let selected = match self.chat_list.selected_index() {
            Some(i) => i,
            None => return,
        };

        if let Some((_id, was_active)) = self.conversations.delete_conversation(selected) {
            self.chat_list.remove_selected();

            if was_active {
                let mut view = ChatView::new();
                view.set_show_timestamps(self.config.ui.show_timestamps);
                self.chat_view = view;

                // Switch to another conversation if available
                if let Some(idx) = self.chat_list.selected_index() {
                    self.switch_to_conversation(idx);
                }
            }
        }
    }

    /// Save the active conversation to the store.
    fn save_active_conversation(&self) {
        self.conversations.save_active_conversation();
    }

    /// Update the active conversation's title based on content.
    fn update_conversation_title(&mut self) {
        if let Some((idx, new_title)) = self.conversations.update_conversation_title() {
            self.chat_list.update_item(idx, new_title);
        }
    }

    /// Process an action. Returns quickly; LLM streaming happens in background.
    pub async fn update(&mut self, action: Action) {
        if self.model_popup.visible && self.handle_popup_action(&action) {
            return;
        }

        match action {
            Action::Quit => {
                self.save_active_conversation();
                self.running = false;
            }
            Action::SwitchMode(mode) => {
                // Gate search mode on feature flag
                if mode == Mode::Search && !self.config.features.search {
                    return;
                }
                // Exiting search mode: clear search state
                if self.mode == Mode::Search && mode != Mode::Search {
                    self.chat_view.clear_search();
                    self.input_box.take_content();
                }
                self.mode = mode;
                self.input_box.handle_action(&Action::SwitchMode(mode));
                self.status_bar.handle_action(&Action::SwitchMode(mode));
                if mode == Mode::Insert || mode == Mode::Command || mode == Mode::Search {
                    self.focus = FocusTarget::Input;
                }
            }
            Action::FocusNext => {
                self.focus = self.focus.next();
            }
            Action::FocusPrev => {
                self.focus = self.focus.prev();
            }
            Action::SendMessage => {
                let content = self.input_box.take_content();
                if !content.trim().is_empty() {
                    let trimmed = content.trim().to_string();

                    // Create a conversation if none active
                    if self.conversations.active_conversation.is_none() {
                        self.create_new_conversation();
                    }

                    // Add user message to chat view
                    self.chat_view.add_message(ChatMessage {
                        role: MessageRole::User,
                        content: trimmed.clone(),
                        timestamp: Some(chrono::Local::now()),
                    });

                    // Add to conversation
                    self.conversations.add_message(Message::user(&trimmed));

                    // Auto-title after first message
                    self.update_conversation_title();
                    self.save_active_conversation();

                    // Start LLM streaming
                    self.start_streaming();
                }
            }
            Action::InputSubmit => {
                if self.mode == Mode::Command {
                    let content = self.input_box.take_content();
                    self.execute_command(&content);
                    self.mode = Mode::Normal;
                    self.input_box.handle_action(&Action::SwitchMode(Mode::Normal));
                    self.status_bar
                        .handle_action(&Action::SwitchMode(Mode::Normal));
                }
            }
            Action::InsertChar(_) | Action::DeleteChar => {
                self.input_box.handle_action(&action);
                if self.mode == Mode::Search {
                    let query = self.input_box.content.clone();
                    self.chat_view.set_search_query(query);
                    if let Some((current, total)) = self.chat_view.search_status() {
                        self.status_bar
                            .set_status(format!("{current}/{total} matches"));
                    } else if !self.input_box.content.is_empty() {
                        self.status_bar.set_status("No matches".to_string());
                    } else {
                        self.status_bar.status_message = None;
                    }
                }
            }
            Action::ScrollUp | Action::ScrollDown => {
                self.dispatch_to_focused(&action);
            }
            Action::NewChat => {
                if self.conversations.store.is_some() {
                    self.create_new_conversation();
                } else {
                    self.chat_list.handle_action(&action);
                }
            }
            Action::DeleteChat => {
                if self.conversations.store.is_some() {
                    self.delete_selected_conversation();
                } else {
                    self.chat_list.handle_action(&action);
                }
            }
            Action::SelectItem => {
                if let Some(idx) = self.chat_list.selected_index()
                    && self.conversations.store.is_some()
                {
                    self.switch_to_conversation(idx);
                }
            }
            Action::ToggleHelp => {
                self.help_overlay.handle_action(&action);
            }
            Action::ToggleModelSelector => {
                if self.model_popup.visible {
                    self.model_popup.close();
                } else {
                    self.model_popup.open(
                        &self.registry,
                        &self.config.general.default_provider,
                        &self.config.general.default_model,
                    );
                }
            }
            Action::SelectModel => {
                // Handled via popup routing guard above
            }
            Action::CopySelection => {
                self.copy_last_response();
            }
            Action::SearchNext => {
                if let Some((current, total)) = self.chat_view.search_next() {
                    self.status_bar
                        .set_status(format!("{current}/{total} matches"));
                }
            }
            Action::SearchPrev => {
                if let Some((current, total)) = self.chat_view.search_prev() {
                    self.status_bar
                        .set_status(format!("{current}/{total} matches"));
                }
            }
            Action::Tick => {
                self.drain_stream_chunks();
            }
            Action::Resize(_, _) | Action::None => {}
        }
    }

    /// Copy the last assistant response content to the system clipboard.
    fn copy_last_response(&mut self) {
        let content = match self
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
        };

        match copy_to_clipboard(&content) {
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

    /// Execute a parsed command from command mode.
    fn execute_command(&mut self, input: &str) {
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
                    self.status_bar.set_status("Contexts feature is disabled in config".to_string());
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
                    self.status_bar.set_status("Contexts feature is disabled in config".to_string());
                } else {
                    self.active_context = None;
                    self.model_selector.context_name = None;
                    if let Some(conv) = &mut self.conversations.active_conversation {
                        conv.context_name = None;
                    }
                    self.status_bar.set_status("Context cleared".to_string());
                }
            }
            Command::Unknown(cmd) => {
                self.status_bar
                    .set_status(format!("Unknown command: {cmd}"));
            }
        }
    }

    /// Handle actions when the model popup is visible. Returns true if consumed.
    fn handle_popup_action(&mut self, action: &Action) -> bool {
        match action {
            Action::ScrollUp
            | Action::ScrollDown
            | Action::SelectItem
            | Action::ToggleModelSelector => {
                let follow_up = self.model_popup.handle_action(action);
                if let Some(Action::SelectModel) = follow_up {
                    if let Some((provider, model)) = self.model_popup.selected_entry() {
                        self.set_active_model(provider, model);
                    }
                    self.model_popup.close();
                }
                true
            }
            Action::SwitchMode(Mode::Normal) => {
                self.model_popup.close();
                true
            }
            Action::Quit | Action::Tick => false, // pass through
            _ => true,                            // consume other actions
        }
    }

    /// Update the active model and provider across config, selector, conversation, and status bar.
    fn set_active_model(&mut self, provider: String, model: String) {
        self.config.general.default_provider = provider.clone();
        self.config.general.default_model = model.clone();
        let mut ms = ModelSelector::new(provider.clone(), model.clone());
        ms.context_name = self.active_context.clone();
        self.model_selector = ms;

        // Update the active conversation's model/provider
        if let Some(conv) = &mut self.conversations.active_conversation {
            conv.provider = provider.clone();
            conv.model = model.clone();
            self.save_active_conversation();
        }

        self.status_bar
            .set_status(format!("Model: {provider}/{model}"));
    }

    /// Find which provider has a model with the given ID.
    fn find_model(&self, model_id: &str) -> Option<String> {
        for provider_name in self.registry.list_providers() {
            if let Some(provider) = self.registry.get(provider_name)
                && provider
                    .available_models()
                    .iter()
                    .any(|m| m.id == model_id)
            {
                return Some(provider_name.to_string());
            }
        }
        None
    }

    /// Spawn a background task to stream LLM response.
    /// When MCP tools are available, runs a tool call loop:
    /// call LLM → if tool calls → execute tools → call LLM again → repeat.
    fn start_streaming(&mut self) {
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

        // Prepend global system prompt if configured
        let mut messages = Vec::new();
        if let Some(ref prompt) = self.config.general.system_prompt {
            messages.push(Message::system(prompt.clone()));
        }

        // Prepend context messages if a context is active
        if let Some(ref ctx_name) = self.active_context {
            if let Some(ctx) = self.contexts.get(ctx_name) {
                messages.extend(ctx.build_messages());
            }
        }
        messages.extend(self.messages().iter().cloned());

        // Attach MCP tools to the request if available
        let tool_defs = self
            .mcp_manager
            .as_ref()
            .filter(|m| m.has_tools())
            .map(|m| m.tool_definitions())
            .unwrap_or_default();

        let mut request = ChatRequest::new(model.clone(), messages).with_tools(tool_defs);
        if let Some(temp) = self.config.general.temperature {
            request = request.with_temperature(temp);
        }
        if let Some(max) = self.config.general.max_tokens {
            request = request.with_max_tokens(max);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        self.stream_rx = Some(rx);
        self.streaming = true;
        self.last_usage = None;

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
                tx.send(StreamChunk::Error(format!("No provider: {provider_name}"))).ok();
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
                        StreamChunk::ToolCallStart { id, name, arguments } => {
                            // Forward to UI for display
                            tx.send(StreamChunk::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            }).ok();
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
                    current_request.messages.push(Message::tool_use(tool_calls.clone()));

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
                        }).ok();

                        // Add tool result message to conversation
                        current_request.messages.push(
                            Message::tool_result(&tc.id, &result.content),
                        );
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
    fn drain_stream_chunks(&mut self) {
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
                    current.input_tokens += usage.input_tokens;
                    current.output_tokens += usage.output_tokens;
                }
                Ok(StreamChunk::ToolCallStart { name, .. }) => {
                    self.chat_view
                        .append_to_last(&format!("\n[Calling tool: {name}...]"));
                    self.status_bar
                        .set_status(format!("calling tool: {name}..."));
                }
                Ok(StreamChunk::ToolCallResult { content, is_error, .. }) => {
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

    fn finish_streaming(&mut self) {
        self.streaming = false;
        self.stream_rx = None;

        // Save the assistant response to the conversation
        if let Some(last) = self.chat_view.messages.last()
            && last.role == MessageRole::Assistant
        {
            self.conversations
                .add_message(Message::assistant(&last.content));
        }

        self.save_active_conversation();

        // Build status with token usage if available
        let status = if let Some(usage) = self.last_usage.take() {
            self.session_usage.input_tokens += usage.input_tokens;
            self.session_usage.output_tokens += usage.output_tokens;
            format!(
                "ready | {} (session: {})",
                usage, self.session_usage
            )
        } else {
            "ready".to_string()
        };
        self.status_bar.set_status(status);
    }

    fn dispatch_to_focused(&mut self, action: &Action) {
        match self.focus {
            FocusTarget::ChatList => {
                self.chat_list.handle_action(action);
            }
            FocusTarget::ChatView => {
                self.chat_view.handle_action(action);
            }
            FocusTarget::ToolPanel => {
                self.tool_panel.handle_action(action);
            }
            FocusTarget::Input => {}
        }
    }
}

/// Copy text to the system clipboard.
///
/// Tries arboard (native OS clipboard) first, then falls back to the OSC 52
/// escape sequence which works in terminals that support it (including over SSH).
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    // Try native clipboard via arboard
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(text) {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::debug!("arboard set_text failed: {e}, trying OSC 52");
            }
        },
        Err(e) => {
            tracing::debug!("arboard init failed: {e}, trying OSC 52");
        }
    }

    // Fallback: OSC 52 escape sequence
    // Works in xterm, kitty, alacritty, iTerm2, Windows Terminal, tmux, etc.
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let osc = format!("\x1b]52;c;{encoded}\x07");
    std::io::stdout()
        .write_all(osc.as_bytes())
        .map_err(|e| format!("clipboard unavailable: {e}"))?;
    std::io::stdout()
        .flush()
        .map_err(|e| format!("clipboard unavailable: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::Conversation;
    use tempfile::TempDir;

    fn test_app() -> App {
        App::new(AppConfig::default(), ProviderRegistry::new())
    }

    fn test_app_with_store() -> (App, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = JsonStore::new(tmp.path()).unwrap();
        let app = App::new(AppConfig::default(), ProviderRegistry::new()).with_store(store);
        (app, tmp)
    }

    #[tokio::test]
    async fn new_app_is_running_in_normal_mode() {
        let app = test_app();
        assert!(app.running);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.focus, FocusTarget::ChatView);
        assert!(!app.streaming);
        assert!(app.messages().is_empty());
        assert!(app.conversations.active_conversation.is_none());
    }

    #[tokio::test]
    async fn quit_action_stops_app() {
        let mut app = test_app();
        app.update(Action::Quit).await;
        assert!(!app.running);
    }

    #[tokio::test]
    async fn switch_to_insert_mode_focuses_input() {
        let mut app = test_app();
        app.update(Action::SwitchMode(Mode::Insert)).await;
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.focus, FocusTarget::Input);
    }

    #[tokio::test]
    async fn focus_next_cycles_panels() {
        let mut app = test_app();
        app.update(Action::FocusNext).await;
        assert_eq!(app.focus, FocusTarget::ToolPanel);
    }

    #[tokio::test]
    async fn focus_prev_cycles_backwards() {
        let mut app = test_app();
        app.update(Action::FocusPrev).await;
        assert_eq!(app.focus, FocusTarget::ChatList);
    }

    #[tokio::test]
    async fn send_message_adds_to_chat() {
        let mut app = test_app();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        assert_eq!(app.chat_view.messages.len(), 1);
        assert_eq!(app.chat_view.messages[0].content, "hi");
        assert_eq!(app.chat_view.messages[0].role, MessageRole::User);
        assert!(app.chat_view.messages[0].timestamp.is_some());
        assert!(app.input_box.content.is_empty());
    }

    #[tokio::test]
    async fn send_empty_message_does_nothing() {
        let mut app = test_app();
        app.update(Action::SendMessage).await;
        assert!(app.chat_view.messages.is_empty());
    }

    #[tokio::test]
    async fn send_whitespace_only_message_does_nothing() {
        let mut app = test_app();
        app.update(Action::InsertChar(' ')).await;
        app.update(Action::SendMessage).await;
        assert!(app.chat_view.messages.is_empty());
    }

    #[tokio::test]
    async fn insert_and_delete_chars() {
        let mut app = test_app();
        app.update(Action::InsertChar('a')).await;
        app.update(Action::InsertChar('b')).await;
        assert_eq!(app.input_box.content, "ab");
        app.update(Action::DeleteChar).await;
        assert_eq!(app.input_box.content, "a");
    }

    #[tokio::test]
    async fn scroll_dispatches_to_focused_panel() {
        let mut app = test_app();
        app.update(Action::ScrollUp).await;
        assert_eq!(app.chat_view.scroll_offset, 1);
    }

    #[tokio::test]
    async fn toggle_help_overlay() {
        let mut app = test_app();
        app.update(Action::ToggleHelp).await;
        assert!(app.help_overlay.visible);
        app.update(Action::ToggleHelp).await;
        assert!(!app.help_overlay.visible);
    }

    #[tokio::test]
    async fn model_selector_reflects_config() {
        let mut config = AppConfig::default();
        config.general.default_provider = "anthropic".to_string();
        config.general.default_model = "claude-sonnet-4-20250514".to_string();
        let app = App::new(config, ProviderRegistry::new());
        assert_eq!(app.model_selector.provider, "anthropic");
        assert_eq!(app.model_selector.model, "claude-sonnet-4-20250514");
    }

    #[tokio::test]
    async fn drain_stream_chunks_appends_deltas() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hello".to_string())).unwrap();
        tx.send(StreamChunk::Delta(" world".to_string())).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        assert_eq!(
            app.chat_view.messages.last().unwrap().content,
            "Hello world"
        );
        assert!(!app.streaming);
    }

    #[tokio::test]
    async fn drain_stream_handles_error() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("partial".to_string())).unwrap();
        tx.send(StreamChunk::Error("rate limited".to_string()))
            .unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        let last = app.chat_view.messages.last().unwrap();
        assert!(last.content.contains("partial"));
        assert!(last.content.contains("[Error: rate limited]"));
        assert!(!app.streaming);
    }

    #[tokio::test]
    async fn drain_stream_handles_disconnect() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("hi".to_string())).unwrap();
        drop(tx);

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();
        assert!(!app.streaming);
    }

    #[tokio::test]
    async fn send_message_without_provider_shows_status() {
        let mut app = test_app();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        assert!(app.status_bar.status_message.is_some());
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("No provider"));
    }

    #[tokio::test]
    async fn tick_drains_stream_chunks() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("tick test".to_string()))
            .unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.update(Action::Tick).await;

        assert_eq!(
            app.chat_view.messages.last().unwrap().content,
            "tick test"
        );
        assert!(!app.streaming);
    }

    #[tokio::test]
    async fn copy_selection_copies_last_assistant() {
        let mut app = test_app();
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "Hello from assistant".to_string(),
            timestamp: None,
        });
        app.update(Action::CopySelection).await;
        assert!(app
            .status_bar
            .status_message
            .as_ref()
            .unwrap()
            .contains("Copied:"));
    }

    #[tokio::test]
    async fn copy_selection_no_assistant_shows_error() {
        let mut app = test_app();
        app.update(Action::CopySelection).await;
        assert!(app
            .status_bar
            .status_message
            .as_ref()
            .unwrap()
            .contains("No assistant message"));
    }

    #[tokio::test]
    async fn theme_is_loaded_from_config() {
        let app = test_app();
        // Default theme should have cyan borders
        assert_eq!(app.theme.border_focused, ratatui::style::Color::Cyan);
    }

    #[tokio::test]
    async fn show_timestamps_propagated_to_chat_view() {
        let mut config = AppConfig::default();
        config.ui.show_timestamps = true;
        let app = App::new(config, ProviderRegistry::new());
        assert!(app.chat_view.show_timestamps);
    }

    // ── Store integration tests ──

    #[tokio::test]
    async fn app_with_store_starts_with_empty_list() {
        let (app, _tmp) = test_app_with_store();
        assert!(app.conversations.store.is_some());
        assert!(app.conversations.conversation_ids.is_empty());
        assert!(app.chat_list.items.is_empty());
    }

    #[tokio::test]
    async fn new_chat_creates_conversation_with_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        assert_eq!(app.chat_list.items.len(), 1);
        assert_eq!(app.conversations.conversation_ids.len(), 1);
        assert!(app.conversations.active_conversation.is_some());
        assert_eq!(app.conversations.active_conversation.as_ref().unwrap().title, "New Chat");
    }

    #[tokio::test]
    async fn new_chat_persists_to_disk() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        let store = app.conversations.store.as_ref().unwrap();
        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
    }

    #[tokio::test]
    async fn send_message_creates_conversation_if_none() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        assert!(app.conversations.active_conversation.is_some());
        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].content, "hi");
    }

    #[tokio::test]
    async fn send_message_auto_titles_conversation() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('H')).await;
        app.update(Action::InsertChar('e')).await;
        app.update(Action::InsertChar('l')).await;
        app.update(Action::InsertChar('p')).await;
        app.update(Action::SendMessage).await;

        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert_eq!(conv.title, "Help");
        assert_eq!(app.chat_list.items[0], "Help");
    }

    #[tokio::test]
    async fn send_message_saves_to_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        let id = app.conversations.active_conversation.as_ref().unwrap().id;
        let store = app.conversations.store.as_ref().unwrap();
        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[tokio::test]
    async fn delete_chat_removes_from_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;
        let id = app.conversations.conversation_ids[0];

        app.update(Action::DeleteChat).await;

        assert!(app.conversations.conversation_ids.is_empty());
        assert!(app.conversations.active_conversation.is_none());

        let store = app.conversations.store.as_ref().unwrap();
        assert!(store.load(id).is_err());
    }

    #[tokio::test]
    async fn switch_conversation_loads_messages() {
        let (mut app, _tmp) = test_app_with_store();

        // Create first chat with a message
        app.update(Action::InsertChar('A')).await;
        app.update(Action::SendMessage).await;

        // Create second chat with a different message
        app.update(Action::NewChat).await;
        app.update(Action::InsertChar('B')).await;
        app.update(Action::SendMessage).await;

        // Switch back to first conversation (index 1 since newest is first)
        app.switch_to_conversation(1);

        assert_eq!(app.chat_view.messages.len(), 1);
        assert_eq!(app.chat_view.messages[0].content, "A");
    }

    #[tokio::test]
    async fn quit_saves_active_conversation() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('x')).await;
        app.update(Action::SendMessage).await;

        let id = app.conversations.active_conversation.as_ref().unwrap().id;
        app.update(Action::Quit).await;

        let store = app.conversations.store.as_ref().unwrap();
        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[tokio::test]
    async fn finish_streaming_saves_assistant_response() {
        let (mut app, _tmp) = test_app_with_store();

        // Create a conversation and simulate streaming
        app.create_new_conversation();
        let user_msg = Message::user("hi");
        app.conversations.active_conversation = Some(
            app.conversations.active_conversation
                .as_ref()
                .unwrap()
                .add_message(user_msg),
        );

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hello!".to_string())).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        // Assistant response should be saved
        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert_eq!(conv.messages.len(), 2); // user + assistant
        assert_eq!(conv.messages[1].content, "Hello!");

        // Should be persisted to store
        let store = app.conversations.store.as_ref().unwrap();
        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
    }

    #[tokio::test]
    async fn app_loads_existing_conversations_on_init() {
        let tmp = TempDir::new().unwrap();
        let store = JsonStore::new(tmp.path()).unwrap();

        // Pre-populate store
        let conv1 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Existing Chat");
        store.save(&conv1).unwrap();

        // Create app with that store
        let app =
            App::new(AppConfig::default(), ProviderRegistry::new()).with_store(store);

        assert_eq!(app.chat_list.items.len(), 1);
        assert_eq!(app.chat_list.items[0], "Existing Chat");
        assert_eq!(app.conversations.conversation_ids.len(), 1);
    }

    #[tokio::test]
    async fn set_active_model_updates_conversation() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        assert_eq!(
            app.conversations.active_conversation.as_ref().unwrap().provider,
            "openai"
        );

        app.set_active_model("anthropic".to_string(), "claude".to_string());

        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert_eq!(conv.provider, "anthropic");
        assert_eq!(conv.model, "claude");
        assert_eq!(app.model_selector.provider, "anthropic");
        assert_eq!(app.model_selector.model, "claude");
    }

    #[tokio::test]
    async fn switch_conversation_restores_model() {
        let (mut app, _tmp) = test_app_with_store();

        // Create first chat (uses default openai/gpt-4o)
        app.update(Action::InsertChar('A')).await;
        app.update(Action::SendMessage).await;

        // Create second chat and switch its model
        app.update(Action::NewChat).await;
        app.set_active_model("anthropic".to_string(), "claude".to_string());

        // Switch back to first conversation
        app.switch_to_conversation(1);

        assert_eq!(app.model_selector.provider, "openai");
        assert_eq!(app.model_selector.model, "gpt-4o");
        assert_eq!(app.config.general.default_provider, "openai");
        assert_eq!(app.config.general.default_model, "gpt-4o");
    }

    #[tokio::test]
    async fn set_active_model_status_shows_provider_and_model() {
        let mut app = test_app();
        app.set_active_model("ollama".to_string(), "llama3".to_string());
        assert!(app.status_bar.status_message.as_ref().unwrap().contains("ollama"));
        assert!(app.status_bar.status_message.as_ref().unwrap().contains("llama3"));
    }

    // ── Token usage tracking ──

    #[tokio::test]
    async fn usage_tracked_after_streaming() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hi".to_string())).unwrap();
        tx.send(StreamChunk::Usage(TokenUsage::new(10, 20))).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        // last_usage consumed by finish_streaming
        assert!(app.last_usage.is_none());
        assert_eq!(app.session_usage.input_tokens, 10);
        assert_eq!(app.session_usage.output_tokens, 20);
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("10in"));
        assert!(status.contains("20out"));
    }

    #[tokio::test]
    async fn session_usage_accumulates_across_requests() {
        let mut app = test_app();

        // First request
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Usage(TokenUsage::new(10, 20))).unwrap();
        tx.send(StreamChunk::Done).unwrap();
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;
        app.drain_stream_chunks();

        // Second request
        let (tx2, rx2) = mpsc::unbounded_channel();
        tx2.send(StreamChunk::Usage(TokenUsage::new(15, 30))).unwrap();
        tx2.send(StreamChunk::Done).unwrap();
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx2);
        app.streaming = true;
        app.drain_stream_chunks();

        assert_eq!(app.session_usage.input_tokens, 25);
        assert_eq!(app.session_usage.output_tokens, 50);
        assert_eq!(app.session_usage.total(), 75);
    }

    #[tokio::test]
    async fn partial_usage_accumulates_anthropic_style() {
        let mut app = test_app();

        // Anthropic sends input_tokens in message_start, output_tokens in message_delta
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Usage(TokenUsage::new(42, 0))).unwrap();
        tx.send(StreamChunk::Delta("Hello".to_string())).unwrap();
        tx.send(StreamChunk::Usage(TokenUsage::new(0, 87))).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        assert_eq!(app.session_usage.input_tokens, 42);
        assert_eq!(app.session_usage.output_tokens, 87);
    }

    #[tokio::test]
    async fn no_usage_shows_plain_ready() {
        let mut app = test_app();

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hello".to_string())).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        assert_eq!(
            app.status_bar.status_message.as_deref(),
            Some("ready")
        );
        assert_eq!(app.session_usage.total(), 0);
    }
}
