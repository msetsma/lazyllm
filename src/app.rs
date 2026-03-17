use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::types::AppConfig;
use crate::event::types::{Action, FocusTarget, Mode};
use crate::llm::ProviderRegistry;
use crate::llm::types::{ChatRequest, Message, StreamChunk};
use crate::store::json_store::JsonStore;
use crate::store::types::Conversation;
use crate::ui::components::chat_list::ChatList;
use crate::ui::components::chat_view::{ChatMessage, ChatView, MessageRole};
use crate::ui::components::help_overlay::HelpOverlay;
use crate::ui::components::input_box::InputBox;
use crate::ui::components::model_selector::ModelSelector;
use crate::ui::components::status_bar::StatusBar;
use crate::ui::components::tool_panel::ToolPanel;
use crate::ui::components::Component;

/// Central application state.
pub struct App {
    pub mode: Mode,
    pub focus: FocusTarget,
    pub running: bool,
    pub streaming: bool,
    pub config: AppConfig,

    // LLM
    pub registry: Arc<ProviderRegistry>,
    pub stream_rx: Option<mpsc::UnboundedReceiver<StreamChunk>>,

    // Persistence
    pub store: Option<JsonStore>,
    pub active_conversation: Option<Conversation>,
    /// IDs parallel to chat_list.items for mapping selection -> conversation
    pub conversation_ids: Vec<Uuid>,

    // Components
    pub chat_list: ChatList,
    pub chat_view: ChatView,
    pub input_box: InputBox,
    pub model_selector: ModelSelector,
    pub status_bar: StatusBar,
    pub tool_panel: ToolPanel,
    pub help_overlay: HelpOverlay,
}

/// Read-only access to the message history for the active conversation.
impl App {
    pub fn messages(&self) -> &[Message] {
        self.active_conversation
            .as_ref()
            .map(|c| c.messages.as_slice())
            .unwrap_or(&[])
    }
}

impl App {
    pub fn new(config: AppConfig, registry: ProviderRegistry) -> Self {
        let model_selector = ModelSelector::new(
            config.general.default_provider.clone(),
            config.general.default_model.clone(),
        );

        Self {
            mode: Mode::Normal,
            focus: FocusTarget::default(),
            running: true,
            streaming: false,
            config,
            registry: Arc::new(registry),
            stream_rx: None,
            store: None,
            active_conversation: None,
            conversation_ids: Vec::new(),
            chat_list: ChatList::new(),
            chat_view: ChatView::new(),
            input_box: InputBox::new(),
            model_selector,
            status_bar: StatusBar::new(),
            tool_panel: ToolPanel::new(),
            help_overlay: HelpOverlay::new(),
        }
    }

    /// Initialize with a store, loading existing conversations.
    pub fn with_store(mut self, store: JsonStore) -> Self {
        self.store = Some(store);
        self.load_conversation_list();
        self
    }

    /// Load conversation summaries from the store into the chat list.
    fn load_conversation_list(&mut self) {
        let store = match &self.store {
            Some(s) => s,
            None => return,
        };

        match store.list() {
            Ok(summaries) => {
                let titles: Vec<String> = summaries.iter().map(|s| s.title.clone()).collect();
                let ids: Vec<Uuid> = summaries.iter().map(|s| s.id).collect();
                self.chat_list = ChatList::from_items(titles);
                self.conversation_ids = ids;
            }
            Err(e) => {
                tracing::warn!("Failed to load conversations: {e}");
            }
        }
    }

    /// Switch to a conversation by index in the chat list.
    fn switch_to_conversation(&mut self, index: usize) {
        // Save current conversation first
        self.save_active_conversation();

        if let Some(&id) = self.conversation_ids.get(index) {
            if let Some(store) = &self.store {
                match store.load(id) {
                    Ok(conv) => {
                        self.load_conversation_into_view(&conv);
                        self.active_conversation = Some(conv);
                    }
                    Err(e) => {
                        self.status_bar = self
                            .status_bar
                            .with_status(format!("Failed to load: {e}"));
                    }
                }
            }
        }
    }

    /// Load a conversation's messages into the chat view.
    fn load_conversation_into_view(&mut self, conv: &Conversation) {
        let mut chat_view = ChatView::new();
        for msg in &conv.messages {
            let role = match msg.role {
                crate::llm::types::Role::User => MessageRole::User,
                crate::llm::types::Role::Assistant => MessageRole::Assistant,
                crate::llm::types::Role::System => MessageRole::System,
                crate::llm::types::Role::Tool => MessageRole::System,
            };
            chat_view = chat_view.add_message(ChatMessage {
                role,
                content: msg.content.clone(),
            });
        }
        self.chat_view = chat_view;
    }

    /// Create a new conversation and switch to it.
    fn create_new_conversation(&mut self) {
        self.save_active_conversation();

        let conv = Conversation::new(
            self.config.general.default_provider.clone(),
            self.config.general.default_model.clone(),
        );

        // Save to store
        if let Some(store) = &self.store {
            if let Err(e) = store.save(&conv) {
                tracing::warn!("Failed to save new conversation: {e}");
            }
        }

        let id = conv.id;
        let title = conv.title.clone();
        self.active_conversation = Some(conv);
        self.chat_view = ChatView::new();

        // Add to list and select it
        self.conversation_ids.insert(0, id);
        self.chat_list = ChatList::from_items(
            std::iter::once(title)
                .chain(self.chat_list.items.iter().cloned())
                .collect(),
        );
    }

    /// Delete the currently selected conversation.
    fn delete_selected_conversation(&mut self) {
        let selected = match self.chat_list.selected_index() {
            Some(i) => i,
            None => return,
        };

        if selected >= self.conversation_ids.len() {
            return;
        }

        let id = self.conversation_ids[selected];

        // Delete from store
        if let Some(store) = &self.store {
            if let Err(e) = store.delete(id) {
                tracing::warn!("Failed to delete conversation: {e}");
            }
        }

        // Remove from lists
        self.conversation_ids.remove(selected);
        self.chat_list = self.chat_list.remove_selected();

        // If we deleted the active conversation, clear or switch
        if self
            .active_conversation
            .as_ref()
            .is_some_and(|c| c.id == id)
        {
            self.active_conversation = None;
            self.chat_view = ChatView::new();

            // Switch to another conversation if available
            if let Some(idx) = self.chat_list.selected_index() {
                self.switch_to_conversation(idx);
            }
        }
    }

    /// Save the active conversation to the store.
    fn save_active_conversation(&mut self) {
        let conv = match &self.active_conversation {
            Some(c) => c,
            None => return,
        };

        if let Some(store) = &self.store {
            if let Err(e) = store.save(conv) {
                tracing::warn!("Failed to save conversation: {e}");
            }
        }
    }

    /// Update the active conversation's title based on content.
    fn update_conversation_title(&mut self) {
        let conv = match &mut self.active_conversation {
            Some(c) => c,
            None => return,
        };

        if conv.title == "New Chat" && !conv.messages.is_empty() {
            let new_title = conv.auto_title();
            conv.title = new_title.clone();

            // Update the chat list display
            if let Some(idx) = self
                .conversation_ids
                .iter()
                .position(|&id| id == conv.id)
            {
                self.chat_list = self.chat_list.update_item(idx, new_title);
            }
        }
    }

    /// Process an action. Returns quickly; LLM streaming happens in background.
    pub async fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.save_active_conversation();
                self.running = false;
            }
            Action::SwitchMode(mode) => {
                self.mode = mode;
                self.input_box.handle_action(&Action::SwitchMode(mode));
                self.status_bar.handle_action(&Action::SwitchMode(mode));
                if mode == Mode::Insert {
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
                let (cleared, content) = self.input_box.take_content();
                self.input_box = cleared;
                if !content.trim().is_empty() {
                    let trimmed = content.trim().to_string();

                    // Create a conversation if none active
                    if self.active_conversation.is_none() {
                        self.create_new_conversation();
                    }

                    // Add user message to chat view
                    self.chat_view = self.chat_view.add_message(ChatMessage {
                        role: MessageRole::User,
                        content: trimmed.clone(),
                    });

                    // Add to conversation
                    if let Some(conv) = &self.active_conversation {
                        self.active_conversation =
                            Some(conv.add_message(Message::user(&trimmed)));
                    }

                    // Auto-title after first message
                    self.update_conversation_title();
                    self.save_active_conversation();

                    // Start LLM streaming
                    self.start_streaming();
                }
            }
            Action::InsertChar(_) | Action::DeleteChar => {
                self.input_box.handle_action(&action);
            }
            Action::ScrollUp | Action::ScrollDown => {
                self.dispatch_to_focused(&action);
            }
            Action::NewChat => {
                if self.store.is_some() {
                    self.create_new_conversation();
                } else {
                    self.chat_list.handle_action(&action);
                }
            }
            Action::DeleteChat => {
                if self.store.is_some() {
                    self.delete_selected_conversation();
                } else {
                    self.chat_list.handle_action(&action);
                }
            }
            Action::SelectItem => {
                if let Some(idx) = self.chat_list.selected_index() {
                    if self.store.is_some() {
                        self.switch_to_conversation(idx);
                    }
                }
            }
            Action::ToggleHelp => {
                self.help_overlay.handle_action(&action);
            }
            Action::Tick => {
                self.drain_stream_chunks();
            }
            Action::ToggleModelSelector | Action::SelectModel => {
                // TODO Phase 5: model selector popup
            }
            Action::Resize(_, _) | Action::None | Action::InputSubmit => {}
        }
    }

    /// Spawn a background task to stream LLM response.
    fn start_streaming(&mut self) {
        let provider_name = &self.config.general.default_provider;
        let _provider = match self.registry.get(provider_name) {
            Some(p) => p,
            None => {
                self.status_bar = self
                    .status_bar
                    .with_status(format!("No provider: {provider_name}"));
                return;
            }
        };

        let model = self.config.general.default_model.clone();
        let messages = self.messages().to_vec();
        let request = ChatRequest::new(model, messages);

        let (tx, rx) = mpsc::unbounded_channel();
        self.stream_rx = Some(rx);
        self.streaming = true;

        // Add empty assistant message that we'll append chunks to
        self.chat_view = self.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
        });

        self.status_bar = self.status_bar.with_status("streaming...".to_string());

        let registry = Arc::clone(&self.registry);
        let provider_name = provider_name.clone();

        tokio::spawn(async move {
            if let Some(provider) = registry.get(&provider_name) {
                if let Err(e) = provider.chat(request, tx.clone()).await {
                    tx.send(StreamChunk::Error(e.to_string())).ok();
                }
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
                    self.chat_view = self.chat_view.append_to_last(&text);
                }
                Ok(StreamChunk::Done) => {
                    self.finish_streaming();
                    break;
                }
                Ok(StreamChunk::Error(msg)) => {
                    self.chat_view =
                        self.chat_view.append_to_last(&format!("\n[Error: {msg}]"));
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
        if let Some(last) = self.chat_view.messages.last() {
            if last.role == MessageRole::Assistant {
                if let Some(conv) = &self.active_conversation {
                    self.active_conversation =
                        Some(conv.add_message(Message::assistant(&last.content)));
                }
            }
        }

        self.save_active_conversation();
        self.status_bar = self.status_bar.with_status("ready".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(app.active_conversation.is_none());
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

        app.chat_view = app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
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

        app.chat_view = app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
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

        app.chat_view = app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
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

        app.chat_view = app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
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

    // ── Store integration tests ──

    #[tokio::test]
    async fn app_with_store_starts_with_empty_list() {
        let (app, _tmp) = test_app_with_store();
        assert!(app.store.is_some());
        assert!(app.conversation_ids.is_empty());
        assert!(app.chat_list.items.is_empty());
    }

    #[tokio::test]
    async fn new_chat_creates_conversation_with_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        assert_eq!(app.chat_list.items.len(), 1);
        assert_eq!(app.conversation_ids.len(), 1);
        assert!(app.active_conversation.is_some());
        assert_eq!(app.active_conversation.as_ref().unwrap().title, "New Chat");
    }

    #[tokio::test]
    async fn new_chat_persists_to_disk() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;

        let store = app.store.as_ref().unwrap();
        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
    }

    #[tokio::test]
    async fn send_message_creates_conversation_if_none() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        assert!(app.active_conversation.is_some());
        let conv = app.active_conversation.as_ref().unwrap();
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

        let conv = app.active_conversation.as_ref().unwrap();
        assert_eq!(conv.title, "Help");
        assert_eq!(app.chat_list.items[0], "Help");
    }

    #[tokio::test]
    async fn send_message_saves_to_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::InsertChar('h')).await;
        app.update(Action::InsertChar('i')).await;
        app.update(Action::SendMessage).await;

        let id = app.active_conversation.as_ref().unwrap().id;
        let store = app.store.as_ref().unwrap();
        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[tokio::test]
    async fn delete_chat_removes_from_store() {
        let (mut app, _tmp) = test_app_with_store();
        app.update(Action::NewChat).await;
        let id = app.conversation_ids[0];

        app.update(Action::DeleteChat).await;

        assert!(app.conversation_ids.is_empty());
        assert!(app.active_conversation.is_none());

        let store = app.store.as_ref().unwrap();
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

        let id = app.active_conversation.as_ref().unwrap().id;
        app.update(Action::Quit).await;

        let store = app.store.as_ref().unwrap();
        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[tokio::test]
    async fn finish_streaming_saves_assistant_response() {
        let (mut app, _tmp) = test_app_with_store();

        // Create a conversation and simulate streaming
        app.create_new_conversation();
        let user_msg = Message::user("hi");
        app.active_conversation = Some(
            app.active_conversation
                .as_ref()
                .unwrap()
                .add_message(user_msg),
        );

        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(StreamChunk::Delta("Hello!".to_string())).unwrap();
        tx.send(StreamChunk::Done).unwrap();

        app.chat_view = app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
        });
        app.stream_rx = Some(rx);
        app.streaming = true;

        app.drain_stream_chunks();

        // Assistant response should be saved
        let conv = app.active_conversation.as_ref().unwrap();
        assert_eq!(conv.messages.len(), 2); // user + assistant
        assert_eq!(conv.messages[1].content, "Hello!");

        // Should be persisted to store
        let store = app.store.as_ref().unwrap();
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
        assert_eq!(app.conversation_ids.len(), 1);
    }
}
