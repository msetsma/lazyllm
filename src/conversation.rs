use uuid::Uuid;

use crate::llm::types::Message;
use crate::store::Store;
use crate::store::types::Conversation;

/// Manages conversation persistence and state.
pub struct ConversationManager {
    pub store: Option<Box<dyn Store>>,
    pub active_conversation: Option<Conversation>,
    pub conversation_ids: Vec<Uuid>,
}

/// Result of loading a conversation: titles and IDs for the chat list.
pub struct ConversationList {
    pub titles: Vec<String>,
    pub ids: Vec<Uuid>,
}

impl Default for ConversationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationManager {
    pub fn new() -> Self {
        Self {
            store: None,
            active_conversation: None,
            conversation_ids: Vec::new(),
        }
    }

    pub fn with_store(mut self, store: impl Store + 'static) -> Self {
        self.store = Some(Box::new(store));
        self
    }

    /// Load conversation summaries from the store.
    pub fn load_conversation_list(&mut self) -> Option<ConversationList> {
        let store = self.store.as_ref()?;

        match store.list() {
            Ok(summaries) => {
                let titles: Vec<String> = summaries.iter().map(|s| s.title.clone()).collect();
                let ids: Vec<Uuid> = summaries.iter().map(|s| s.id).collect();
                self.conversation_ids = ids.clone();
                Some(ConversationList { titles, ids })
            }
            Err(e) => {
                tracing::warn!("Failed to load conversations: {e}");
                None
            }
        }
    }

    /// Switch to a conversation by index. Returns the conversation's messages
    /// if successful, or an error message.
    pub fn switch_to_conversation(&mut self, index: usize) -> Result<&Conversation, String> {
        self.save_active_conversation();

        let &id = self
            .conversation_ids
            .get(index)
            .ok_or_else(|| "Invalid index".to_string())?;

        let store = self.store.as_ref().ok_or_else(|| "No store".to_string())?;

        match store.load(id) {
            Ok(conv) => {
                self.active_conversation = Some(conv);
                Ok(self.active_conversation.as_ref().unwrap())
            }
            Err(e) => Err(format!("Failed to load: {e}")),
        }
    }

    /// Create a new conversation. Returns the new conversation's ID and title.
    pub fn create_new_conversation(
        &mut self,
        provider: String,
        model: String,
    ) -> (Uuid, String) {
        self.save_active_conversation();

        let conv = Conversation::new(provider, model);

        if let Some(store) = &self.store
            && let Err(e) = store.save(&conv)
        {
            tracing::warn!("Failed to save new conversation: {e}");
        }

        let id = conv.id;
        let title = conv.title.clone();
        self.active_conversation = Some(conv);
        self.conversation_ids.insert(0, id);

        (id, title)
    }

    /// Delete the selected conversation. Returns the ID of the deleted
    /// conversation and whether the active conversation was deleted.
    pub fn delete_conversation(&mut self, selected: usize) -> Option<(Uuid, bool)> {
        if selected >= self.conversation_ids.len() {
            return None;
        }

        let id = self.conversation_ids[selected];

        if let Some(store) = &self.store
            && let Err(e) = store.delete(id)
        {
            tracing::warn!("Failed to delete conversation: {e}");
        }

        self.conversation_ids.remove(selected);

        let was_active = self
            .active_conversation
            .as_ref()
            .is_some_and(|c| c.id == id);

        if was_active {
            self.active_conversation = None;
        }

        Some((id, was_active))
    }

    /// Save the active conversation to the store.
    pub fn save_active_conversation(&self) {
        let conv = match &self.active_conversation {
            Some(c) => c,
            None => return,
        };

        if let Some(store) = &self.store
            && let Err(e) = store.save(conv)
        {
            tracing::warn!("Failed to save conversation: {e}");
        }
    }

    /// Update the active conversation's title if it's still "New Chat".
    /// Returns the new title and the index to update in the chat list, if changed.
    pub fn update_conversation_title(&mut self) -> Option<(usize, String)> {
        let conv = self.active_conversation.as_mut()?;

        if conv.title != "New Chat" || conv.messages.is_empty() {
            return None;
        }

        let new_title = conv.auto_title();
        conv.title = new_title.clone();

        let idx = self
            .conversation_ids
            .iter()
            .position(|&id| id == conv.id)?;

        Some((idx, new_title))
    }

    /// Read-only access to the message history for the active conversation.
    pub fn messages(&self) -> &[Message] {
        self.active_conversation
            .as_ref()
            .map(|c| c.messages.as_slice())
            .unwrap_or(&[])
    }

    /// Add a message to the active conversation.
    pub fn add_message(&mut self, message: Message) {
        if let Some(conv) = &self.active_conversation {
            self.active_conversation = Some(conv.add_message(message));
        }
    }
}
