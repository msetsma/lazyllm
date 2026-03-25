use std::io::Write;
use std::sync::Arc;

use base64::Engine;
use tokio::sync::mpsc;

use crate::config::types::AppConfig;
use crate::context::Context;
use crate::conversation::ConversationManager;
use crate::event::types::{Action, FocusTarget, Mode};
use crate::llm::ProviderRegistry;
use crate::llm::compaction::{self, PipelineResult};
use crate::llm::types::{Message, StreamChunk, TokenUsage};
use crate::mcp::McpManager;
use crate::store::Store;
use crate::ui::components::chat_list::ChatList;
use crate::ui::components::chat_view::{ChatMessage, ChatView, MessageRole};
use crate::ui::components::help_overlay::HelpOverlay;
use crate::ui::components::input_box::InputBox;
use crate::ui::components::model_popup::ModelPopup;
use crate::ui::components::model_selector::ModelSelector;
use crate::ui::components::notes_editor::NotesEditor;
use crate::ui::components::pulse_overlay::PulseOverlay;
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

    // Context management
    pub(crate) compaction_summary: Option<String>,
    pub(crate) compaction_rx: Option<mpsc::UnboundedReceiver<PipelineResult>>,

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
    pub(crate) pulse_overlay: PulseOverlay,
    pub(crate) notes_editor: NotesEditor,
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

    /// Returns true when an overlay is visible that needs raw key events
    /// instead of resolved actions.
    pub fn wants_raw_keys(&self) -> bool {
        self.pulse_overlay.visible || self.notes_editor.visible
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
            compaction_summary: None,
            compaction_rx: None,
            conversations: ConversationManager::new(),
            chat_list: ChatList::new(),
            chat_view,
            input_box: InputBox::new(),
            model_selector: ms,
            status_bar: StatusBar::new(),
            tool_panel: ToolPanel::new(),
            help_overlay: HelpOverlay::new(),
            model_popup: ModelPopup::new(),
            pulse_overlay: PulseOverlay::new(),
            notes_editor: NotesEditor::new(),
        }
    }

    /// Initialize with a store, loading existing conversations.
    pub fn with_store(mut self, store: impl Store + 'static) -> Self {
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
    pub(crate) fn switch_to_conversation(&mut self, index: usize) {
        match self.conversations.switch_to_conversation(index) {
            Ok(conv) => {
                // Restore the model/provider from the conversation
                self.config.general.default_provider = conv.provider.clone();
                self.config.general.default_model = conv.model.clone();
                let mut ms = ModelSelector::new(conv.provider.clone(), conv.model.clone());
                ms.mcp_server_count = self.model_selector.mcp_server_count;

                // Restore context from conversation
                self.active_context = conv.context_name.clone();
                ms.context_name = conv.context_name.clone();
                self.model_selector = ms;

                // Reset compaction state for the new conversation
                self.compaction_summary = None;

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
    pub(crate) fn create_new_conversation(&mut self) {
        let provider = self.config.general.default_provider.clone();
        let model = self.config.general.default_model.clone();
        let (_id, title) = self.conversations.create_new_conversation(provider, model);

        // Reset context management state
        self.compaction_summary = None;
        self.model_selector.context_usage_pct = None;

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
    pub(crate) fn delete_selected_conversation(&mut self) {
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
    pub(crate) fn save_active_conversation(&self) {
        self.conversations.save_active_conversation();
    }

    /// Update the active conversation's title based on content.
    pub(crate) fn update_conversation_title(&mut self) {
        if let Some((idx, new_title)) = self.conversations.update_conversation_title() {
            self.chat_list.update_item(idx, new_title);
        }
    }

    /// Process an action. Returns quickly; LLM streaming happens in background.
    pub async fn update(&mut self, action: Action) {
        // Raw key interception for overlays that need their own key mapping
        if let Action::RawKey(key_event) = action {
            use crate::ui::components::notes_editor::NotesAction;

            // Notes editor takes priority (topmost popup)
            if self.notes_editor.visible {
                match self.notes_editor.handle_key(key_event) {
                    NotesAction::Save => {
                        let notes = self.notes_editor.close_save();
                        if let Some(conv) = &mut self.conversations.active_conversation {
                            conv.session_notes =
                                if notes.is_empty() { None } else { Some(notes) };
                        }
                        self.save_active_conversation();
                        self.refresh_pulse_data();
                    }
                    NotesAction::Discard => {
                        self.notes_editor.close_discard();
                    }
                    NotesAction::Continue => {}
                }
                return;
            }

            // Pulse overlay intercepts all keys when visible
            if self.pulse_overlay.visible {
                if let Some(overlay_action) = self.pulse_overlay.handle_key(key_event) {
                    // Recurse with the resolved action so the main dispatch handles it
                    Box::pin(self.update(overlay_action)).await;
                }
                return;
            }

            // Shouldn't happen (wants_raw_keys was true but no overlay visible),
            // but fall through to resolve normally just in case
            let resolved =
                crate::event::keybindings::resolve_key(key_event, self.mode, self.focus);
            Box::pin(self.update(resolved)).await;
            return;
        }

        if self.model_popup.visible && self.handle_popup_action(&action) {
            return;
        }

        if self.help_overlay.visible && self.handle_help_overlay_action(&action) {
            return;
        }

        if self.tool_panel.visible && self.handle_tool_panel_action(&action) {
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
                // Entering Visual mode: focus the chat view, highlight copy target
                if mode == Mode::Visual {
                    self.focus = FocusTarget::ChatView;
                    self.chat_view.enter_visual_mode();
                }
                // Leaving Visual mode: snap back to bottom and return focus to input
                if self.mode == Mode::Visual && mode != Mode::Visual {
                    self.chat_view.exit_visual_mode();
                    self.focus = FocusTarget::Input;
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
                if self.mode == Mode::Visual {
                    // Visual mode always scrolls the chat view
                    self.chat_view.handle_action(&action);
                } else {
                    self.dispatch_to_focused(&action);
                }
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
            Action::ToggleToolPanel => {
                self.tool_panel.handle_action(&action);
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
                self.drain_compaction_result();
            }
            Action::TogglePulse => {
                self.pulse_overlay.visible = !self.pulse_overlay.visible;
                if self.pulse_overlay.visible {
                    self.refresh_pulse_data();
                }
            }
            Action::PulseCompact => {
                self.spawn_compaction(None);
                self.refresh_pulse_data();
            }
            Action::PulseCompactWithPrompt => {
                self.pulse_overlay.visible = false;
                self.mode = Mode::Command;
                self.input_box.set_content("compact ");
                self.focus = FocusTarget::Input;
                self.status_bar.handle_action(&Action::SwitchMode(Mode::Command));
            }
            Action::PulseClearToolResults => {
                if let Some(conv) = &mut self.conversations.active_conversation {
                    let pinned = conv.pinned_messages.clone();
                    let result = compaction::clear_old_tool_results(
                        &mut conv.messages, 3, &pinned,
                    );
                    self.status_bar.set_status(format!(
                        "Cleared {} tool results, reclaimed ~{}k tokens",
                        result.cleared_count, result.tokens_reclaimed / 1000,
                    ));
                }
                self.refresh_pulse_data();
            }
            Action::PulseUndoCompaction => {
                self.restore_checkpoint(None);
                self.refresh_pulse_data();
                self.status_bar.set_status("Restored from last checkpoint".to_string());
            }
            Action::PulseTogglePin => {
                if let Some(conv) = &mut self.conversations.active_conversation {
                    let idx = self.chat_view.selected_message_index();
                    if let Some(pos) = conv.pinned_messages.iter().position(|&i| i == idx) {
                        conv.pinned_messages.remove(pos);
                    } else {
                        conv.pinned_messages.push(idx);
                    }
                }
                self.save_active_conversation();
                self.refresh_pulse_data();
            }
            Action::PulseEditNotes => {
                let notes = self.conversations.active_conversation
                    .as_ref()
                    .and_then(|c| c.session_notes.as_deref())
                    .unwrap_or("");
                self.notes_editor.open(notes);
            }
            Action::PulseSwitchMode => {
                let current = &self.config.conversation.compaction_mode;
                let next = match current.as_str() {
                    "auto" => "client",
                    "client" => "server",
                    "server" => "auto",
                    _ => "auto",
                };
                self.config.conversation.compaction_mode = next.to_string();
                self.refresh_pulse_data();
                self.status_bar.set_status(format!("Compaction mode: {next}"));
            }
            Action::Resize(_, _) | Action::None | Action::RawKey(_) => {}
        }
    }

    /// Handle actions when the help overlay is visible. Returns true if consumed.
    fn handle_help_overlay_action(&mut self, action: &Action) -> bool {
        match action {
            Action::SwitchMode(Mode::Normal) | Action::ToggleHelp => {
                self.help_overlay.visible = false;
                true
            }
            Action::Quit | Action::Tick => false, // pass through
            _ => true,                            // consume other actions
        }
    }

    fn handle_tool_panel_action(&mut self, action: &Action) -> bool {
        match action {
            Action::SwitchMode(Mode::Normal) | Action::ToggleToolPanel => {
                self.tool_panel.close();
                true
            }
            Action::Quit | Action::Tick => false, // pass through
            _ => true,                            // consume other actions
        }
    }

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
    pub(crate) fn set_active_model(&mut self, provider: String, model: String) {
        self.config.general.default_provider = provider.clone();
        self.config.general.default_model = model.clone();
        let mut ms = ModelSelector::new(provider.clone(), model.clone());
        ms.mcp_server_count = self.model_selector.mcp_server_count;
        ms.context_name = self.active_context.clone();
        ms.context_usage_pct = self.model_selector.context_usage_pct;
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
    pub(crate) fn find_model(&self, model_id: &str) -> Option<String> {
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

    fn dispatch_to_focused(&mut self, action: &Action) {
        match self.focus {
            FocusTarget::ChatList => {
                self.chat_list.handle_action(action);
            }
            FocusTarget::ChatView => {
                self.chat_view.handle_action(action);
            }
            FocusTarget::Input => {}
        }
    }
}

/// Copy text to the system clipboard.
///
/// Tries arboard (native OS clipboard) first, then falls back to the OSC 52
/// escape sequence which works in terminals that support it (including over SSH).
pub(crate) fn copy_to_clipboard(text: &str) -> Result<(), String> {
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
    use crate::store::sqlite_store::SqliteStore;
    use crate::store::types::Conversation;
    use tempfile::TempDir;

    fn test_app() -> App {
        App::new(AppConfig::default(), ProviderRegistry::new())
    }

    fn test_app_with_store() -> (App, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new(tmp.path()).unwrap();
        let app = App::new(AppConfig::default(), ProviderRegistry::new()).with_store(store);
        (app, tmp)
    }

    /// Verifies the app starts in Normal mode, focused on ChatView, with an empty conversation.
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

    /// Ensures the Quit action sets running to false, stopping the app loop.
    #[tokio::test]
    async fn quit_action_stops_app() {
        let mut app = test_app();
        app.update(Action::Quit).await;
        assert!(!app.running);
    }

    /// Verifies that switching to Insert mode moves focus to the input box.
    #[tokio::test]
    async fn switch_to_insert_mode_focuses_input() {
        let mut app = test_app();
        app.update(Action::SwitchMode(Mode::Insert)).await;
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.focus, FocusTarget::Input);
    }

    /// Ensures FocusNext cycles from ChatView to the next panel (Input).
    #[tokio::test]
    async fn focus_next_cycles_panels() {
        let mut app = test_app();
        app.update(Action::FocusNext).await;
        assert_eq!(app.focus, FocusTarget::Input);
    }

    /// Ensures FocusPrev cycles backwards from ChatView to ChatList.
    #[tokio::test]
    async fn focus_prev_cycles_backwards() {
        let mut app = test_app();
        app.update(Action::FocusPrev).await;
        assert_eq!(app.focus, FocusTarget::ChatList);
    }

    /// Verifies that sending a message adds it to the chat view with correct role and timestamp.
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

    /// Ensures sending an empty message is a no-op (no message added to chat).
    #[tokio::test]
    async fn send_empty_message_does_nothing() {
        let mut app = test_app();
        app.update(Action::SendMessage).await;
        assert!(app.chat_view.messages.is_empty());
    }

    /// Ensures sending a whitespace-only message is a no-op.
    #[tokio::test]
    async fn send_whitespace_only_message_does_nothing() {
        let mut app = test_app();
        app.update(Action::InsertChar(' ')).await;
        app.update(Action::SendMessage).await;
        assert!(app.chat_view.messages.is_empty());
    }

    /// Verifies InsertChar appends characters and DeleteChar removes the last one.
    #[tokio::test]
    async fn insert_and_delete_chars() {
        let mut app = test_app();
        app.update(Action::InsertChar('a')).await;
        app.update(Action::InsertChar('b')).await;
        assert_eq!(app.input_box.content, "ab");
        app.update(Action::DeleteChar).await;
        assert_eq!(app.input_box.content, "a");
    }

    /// Ensures ScrollUp dispatches to the focused panel and increments its scroll offset.
    #[tokio::test]
    async fn scroll_dispatches_to_focused_panel() {
        let mut app = test_app();
        app.update(Action::ScrollUp).await;
        assert_eq!(app.chat_view.scroll_offset.get(), 1);
    }

    /// Verifies ToggleHelp flips the help overlay visibility on and off.
    #[tokio::test]
    async fn toggle_help_overlay() {
        let mut app = test_app();
        app.update(Action::ToggleHelp).await;
        assert!(app.help_overlay.visible);
        app.update(Action::ToggleHelp).await;
        assert!(!app.help_overlay.visible);
    }

    /// Ensures the model selector widget is initialized from config defaults.
    #[tokio::test]
    async fn model_selector_reflects_config() {
        let mut config = AppConfig::default();
        config.general.default_provider = "anthropic".to_string();
        config.general.default_model = "claude-sonnet-4-20250514".to_string();
        let app = App::new(config, ProviderRegistry::new());
        assert_eq!(app.model_selector.provider, "anthropic");
        assert_eq!(app.model_selector.model, "claude-sonnet-4-20250514");
    }

    /// Verifies stream deltas are concatenated into the last assistant message and streaming ends on Done.
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

    /// Ensures stream errors append an error tag to the assistant message and stop streaming.
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

    /// Ensures a dropped sender (disconnect) gracefully stops streaming without panic.
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

    /// Verifies sending a message with no registered provider shows a "No provider" status error.
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

    /// Ensures the Tick action triggers drain_stream_chunks to process pending stream data.
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

    /// Verifies CopySelection copies the last assistant message and shows a status confirmation.
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

    /// Ensures CopySelection with no assistant messages shows an error status.
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

    /// Ensures the theme is loaded from config, defaulting to cyan focused borders.
    #[tokio::test]
    async fn theme_is_loaded_from_config() {
        let app = test_app();
        // Default theme should have cyan borders
        assert_eq!(app.theme.border_focused, ratatui::style::Color::Cyan);
    }

    /// Verifies the show_timestamps config flag is propagated to the chat view widget.
    #[tokio::test]
    async fn show_timestamps_propagated_to_chat_view() {
        let mut config = AppConfig::default();
        config.ui.show_timestamps = true;
        let app = App::new(config, ProviderRegistry::new());
        assert!(app.chat_view.show_timestamps);
    }

    // ── Store integration tests ──

    /// Ensures an app with a fresh store starts with an empty conversation list.
    #[tokio::test]
    async fn app_with_store_starts_with_empty_list() {
        let (app, _tmp) = test_app_with_store();
        assert!(app.conversations.store.is_some());
        assert!(app.conversations.conversation_ids.is_empty());
        assert!(app.chat_list.items.is_empty());
    }

    /// Verifies NewChat creates a conversation entry in both the UI list and the store.
    #[tokio::test]
    async fn new_chat_creates_conversation_with_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        assert_eq!(app.chat_list.items.len(), 1);
        assert_eq!(app.conversations.conversation_ids.len(), 1);
        assert!(app.conversations.active_conversation.is_some());
        assert_eq!(app.conversations.active_conversation.as_ref().unwrap().title, "New Chat");
    }

    /// Ensures a new conversation is persisted to the backing store on disk.
    #[tokio::test]
    async fn new_chat_persists_to_disk() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        let store = app.conversations.store.as_ref().unwrap();
        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
    }

    /// Verifies sending a message auto-creates a conversation when none exists.
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

    /// Ensures the first message auto-titles the conversation using the message content.
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

    /// Verifies sent messages are persisted to the store and can be reloaded by conversation ID.
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

    /// Ensures DeleteChat removes the conversation from both memory and the backing store.
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

    /// Verifies switching conversations loads the correct messages into the chat view.
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

    /// Ensures Quit persists the active conversation to the store before exiting.
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

    /// Verifies completed streaming appends the assistant response to the conversation and persists it.
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

    /// Ensures an app initialized with a pre-populated store loads existing conversations into the UI.
    #[tokio::test]
    async fn app_loads_existing_conversations_on_init() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new(tmp.path()).unwrap();

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

    /// Verifies set_active_model updates the conversation's provider/model and the selector widget.
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

    /// Ensures switching conversations restores the per-conversation model selection.
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

    /// Verifies set_active_model updates the status bar to show the new provider and model.
    #[tokio::test]
    async fn set_active_model_status_shows_provider_and_model() {
        let mut app = test_app();
        app.set_active_model("ollama".to_string(), "llama3".to_string());
        assert!(app.status_bar.status_message.as_ref().unwrap().contains("ollama"));
        assert!(app.status_bar.status_message.as_ref().unwrap().contains("llama3"));
    }

    // ── Token usage tracking ──

    /// Ensures token usage from stream chunks is tracked in session totals and shown in status.
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

    /// Verifies session usage accumulates token counts across multiple streaming requests.
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

    /// Ensures split usage events (Anthropic-style: input first, output later) accumulate correctly.
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

    /// Ensures streams with no usage data show a plain "ready" status.
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

    // ── Context & compaction tests ──

    /// Verifies creating a new conversation resets compaction summary and context usage.
    #[tokio::test]
    async fn new_conversation_clears_compaction_state() {
        let (mut app, _tmp) = test_app_with_store();
        app.compaction_summary = Some("old summary".to_string());
        app.model_selector.context_usage_pct = Some(85);

        app.create_new_conversation();

        assert!(app.compaction_summary.is_none());
        assert!(app.model_selector.context_usage_pct.is_none());
    }

    /// Ensures switching conversations clears the compaction summary from the previous chat.
    #[tokio::test]
    async fn switch_conversation_clears_compaction() {
        let (mut app, _tmp) = test_app_with_store();

        // Create two conversations
        app.update(Action::InsertChar('A')).await;
        app.update(Action::SendMessage).await;
        app.update(Action::NewChat).await;
        app.update(Action::InsertChar('B')).await;
        app.update(Action::SendMessage).await;

        app.compaction_summary = Some("summary".to_string());
        app.switch_to_conversation(1);
        assert!(app.compaction_summary.is_none());
    }

    /// Ensures compacting a conversation shorter than the threshold shows a "too short" status.
    #[tokio::test]
    async fn compact_too_short_shows_message() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();

        // Add fewer messages than recent_messages threshold
        for i in 0..5 {
            app.conversations.add_message(Message::user(format!("msg {i}")));
        }

        app.spawn_compaction(None);
        assert!(app
            .status_bar
            .status_message
            .as_ref()
            .unwrap()
            .contains("too short"));
    }

    /// Verifies drain_compaction_result applies a PipelineResult to the conversation.
    #[tokio::test]
    async fn compact_reduces_messages() {
        use crate::store::types::CompactionMode;

        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();

        // Add 30 messages
        for i in 0..30 {
            app.conversations.add_message(Message::user(format!("msg {i}")));
            app.chat_view.add_message(ChatMessage {
                role: MessageRole::User,
                content: format!("msg {i}"),
                timestamp: None,
            });
        }

        // Simulate a completed pipeline result by sending through the channel
        let (tx, rx) = mpsc::unbounded_channel();
        app.compaction_rx = Some(rx);

        let kept_messages: Vec<Message> = (25..30).map(|i| Message::user(format!("msg {i}"))).collect();
        tx.send(PipelineResult {
            messages: kept_messages,
            summary: Some("Summary of earlier conversation".to_string()),
            total_removed: 25,
            total_tokens_reclaimed: 500,
            steps_applied: vec![CompactionMode::ToolClearing, CompactionMode::Truncation],
        }).unwrap();
        drop(tx);

        app.drain_compaction_result();

        // Verify compaction was applied
        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert_eq!(conv.messages.len(), 5);
        assert_eq!(app.compaction_summary, Some("Summary of earlier conversation".to_string()));
        assert_eq!(conv.compaction_history.len(), 1);
        assert_eq!(conv.compaction_history[0].messages_dropped, 25);
        assert!(app.compaction_rx.is_none());
    }

    /// Verifies should_auto_compact returns false when strategy is "none".
    #[tokio::test]
    async fn auto_compact_disabled_when_strategy_none() {
        let (mut app, _tmp) = test_app_with_store();
        app.config.conversation.compaction_strategy = "none".to_string();
        app.create_new_conversation();
        for i in 0..50 {
            app.conversations.add_message(Message::user(format!("msg {i}")));
        }
        assert!(!app.should_auto_compact());
    }

    /// Verifies should_auto_compact returns false when compaction already in flight.
    #[tokio::test]
    async fn auto_compact_skips_when_in_flight() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        let (_tx, rx) = mpsc::unbounded_channel::<PipelineResult>();
        app.compaction_rx = Some(rx);
        assert!(!app.should_auto_compact());
    }

    /// Verifies spawn_compaction rejects when compaction already in progress.
    #[tokio::test]
    async fn spawn_compaction_rejects_duplicate() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        let (_tx, rx) = mpsc::unbounded_channel::<PipelineResult>();
        app.compaction_rx = Some(rx);

        app.spawn_compaction(None);
        assert!(app.status_bar.status_message.as_ref().unwrap().contains("already in progress"));
    }

    /// Verifies restore_checkpoint clears compaction summary and pops compaction history.
    #[tokio::test]
    async fn restore_checkpoint_clears_state() {
        use crate::store::types::{CompactionEvent, CompactionMode};

        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();

        // Add messages and save
        for i in 0..10 {
            app.conversations.add_message(Message::user(format!("msg {i}")));
        }

        // Save a checkpoint manually
        let conv = app.conversations.active_conversation.as_ref().unwrap();
        if let Some(store) = &app.conversations.store {
            store.save_checkpoint(conv.id, &conv.messages, Some("test")).unwrap();
        }

        // Add a compaction event
        if let Some(conv) = &mut app.conversations.active_conversation {
            conv.compaction_history.push(CompactionEvent {
                timestamp: chrono::Utc::now(),
                mode: CompactionMode::Truncation,
                summary_preview: "test".to_string(),
                messages_before: 10,
                messages_dropped: 5,
                tokens_reclaimed: 100,
                checkpoint_id: None,
            });
        }
        app.compaction_summary = Some("test summary".to_string());

        // Restore latest checkpoint
        app.restore_checkpoint(None);

        assert!(app.compaction_summary.is_none());
        let conv = app.conversations.active_conversation.as_ref().unwrap();
        assert!(conv.compaction_history.is_empty());
        assert_eq!(conv.messages.len(), 10);
    }

    /// Verifies ":set temperature" updates the config and rejects out-of-range values.
    #[tokio::test]
    async fn set_command_temperature() {
        let mut app = test_app();
        app.set_config("temperature", "0.5");
        assert_eq!(app.config.general.temperature, Some(0.5));

        app.set_config("temperature", "3.0");
        // Should not change — out of range
        assert_eq!(app.config.general.temperature, Some(0.5));
    }

    /// Verifies ":set compaction" updates the strategy and rejects invalid values.
    #[tokio::test]
    async fn set_command_compaction() {
        let mut app = test_app();
        app.set_config("compaction", "truncation");
        assert_eq!(app.config.conversation.compaction_strategy, "truncation");

        app.set_config("compaction", "invalid");
        // Should not change
        assert_eq!(app.config.conversation.compaction_strategy, "truncation");
    }

    /// Verifies ":set show_cost" toggles the cost display flag on and off.
    #[tokio::test]
    async fn set_command_show_cost() {
        let mut app = test_app();
        app.set_config("show_cost", "false");
        assert!(!app.config.usage.show_cost);
        app.set_config("show_cost", "true");
        assert!(app.config.usage.show_cost);
    }

    /// Ensures the ":usage" command displays token counts, turn count, and cost in the chat view.
    #[tokio::test]
    async fn usage_command_shows_info() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();

        // Set some usage on the conversation
        if let Some(conv) = &mut app.conversations.active_conversation {
            conv.total_input_tokens = 100;
            conv.total_output_tokens = 200;
            conv.turn_count = 3;
            conv.total_cost = 0.005;
        }

        app.show_usage();
        let last_msg = app.chat_view.messages.last().unwrap();
        assert!(last_msg.content.contains("100in"));
        assert!(last_msg.content.contains("200out"));
        assert!(last_msg.content.contains("3 turns"));
        assert!(last_msg.content.contains("$0.005"));
    }

    /// Verifies the status bar hides cost information when show_cost is disabled.
    #[tokio::test]
    async fn cost_display_respects_show_cost_flag() {
        let mut app = test_app();
        app.config.usage.show_cost = false;
        app.config.usage.show_token_usage = true;

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hi".to_string())).unwrap();
        tx.send(StreamChunk::Usage(TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
            cost: 0.05,
            ..Default::default()
        })).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            timestamp: None,
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("10in"));
        assert!(!status.contains("$")); // cost should be hidden
    }

    /// Ensures the :quit command saves the active conversation and stops the app.
    #[tokio::test]
    async fn execute_command_quit() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        app.execute_command("quit");
        assert!(!app.running);
    }

    /// Ensures the :new command creates a new conversation.
    #[tokio::test]
    async fn execute_command_new_chat() {
        let (mut app, _tmp) = test_app_with_store();
        app.execute_command("new");
        assert!(app.conversations.active_conversation.is_some());
        assert_eq!(app.conversations.conversation_ids.len(), 1);
    }

    /// Ensures the :clear command empties the chat view and conversation messages.
    #[tokio::test]
    async fn execute_command_clear() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        app.conversations.add_message(Message::user("test"));
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "test".to_string(),
            timestamp: None,
        });

        app.execute_command("clear");

        assert!(app.chat_view.messages.is_empty());
        assert!(app.conversations.active_conversation.as_ref().unwrap().messages.is_empty());
    }

    /// Ensures the :help command toggles the help overlay.
    #[tokio::test]
    async fn execute_command_help() {
        let mut app = test_app();
        app.execute_command("help");
        assert!(app.help_overlay.visible);
    }

    /// Ensures an unknown command sets a status bar error message.
    #[tokio::test]
    async fn execute_command_unknown() {
        let mut app = test_app();
        app.execute_command("notacommand");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("Unknown command"));
    }

    /// Ensures the :model command with an unknown model shows an error status.
    #[tokio::test]
    async fn execute_command_model_unknown() {
        let mut app = test_app();
        app.execute_command("model nonexistent-model");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("Unknown model"));
    }

    /// Ensures the :provider command with an unknown provider shows an error status.
    #[tokio::test]
    async fn execute_command_provider_unknown() {
        let mut app = test_app();
        app.execute_command("provider nonexistent-provider");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("Unknown provider"));
    }

    /// Ensures the :context command with a disabled contexts feature shows a status message.
    #[tokio::test]
    async fn execute_command_context_disabled() {
        let mut app = test_app();
        app.config.features.contexts = false;
        app.execute_command("context myctx");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("disabled"));
    }

    /// Ensures the :context command without args clears the active context.
    #[tokio::test]
    async fn execute_command_context_clear() {
        let mut app = test_app();
        app.config.features.contexts = true;
        app.active_context = Some("test".to_string());
        app.execute_command("context");
        assert!(app.active_context.is_none());
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("cleared"));
    }

    /// Ensures the :usage command without active conversation shows an error.
    #[tokio::test]
    async fn execute_command_usage_no_active() {
        let mut app = test_app();
        app.execute_command("usage");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("No active conversation"));
    }

    /// Ensures the :usage command with an active conversation adds a system message with token stats.
    #[tokio::test]
    async fn execute_command_usage_with_active() {
        let (mut app, _tmp) = test_app_with_store();
        app.create_new_conversation();
        app.execute_command("usage");
        let last = app.chat_view.messages.last().unwrap();
        assert_eq!(last.role, MessageRole::System);
        assert!(last.content.contains("Usage:"));
    }

    /// Ensures the :export command without active conversation shows an error.
    #[tokio::test]
    async fn execute_command_export_no_active() {
        let (mut app, _tmp) = test_app_with_store();
        app.execute_command("export");
        let status = app.status_bar.status_message.as_ref().unwrap();
        assert!(status.contains("No active conversation"));
    }

    /// Ensures the :delete command with no conversations is a no-op.
    #[tokio::test]
    async fn execute_command_delete_empty() {
        let (mut app, _tmp) = test_app_with_store();
        app.execute_command("delete");
        assert!(app.conversations.active_conversation.is_none());
    }
}
