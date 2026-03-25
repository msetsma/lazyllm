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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite_store::SqliteStore;
    use tempfile::TempDir;

    fn manager_with_store() -> (ConversationManager, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new(tmp.path()).unwrap();
        let mgr = ConversationManager::new().with_store(store);
        (mgr, tmp)
    }

    /// Ensures a new manager starts with no store, no active conversation, and empty IDs.
    #[test]
    fn new_manager_is_empty() {
        let mgr = ConversationManager::new();
        assert!(mgr.store.is_none());
        assert!(mgr.active_conversation.is_none());
        assert!(mgr.conversation_ids.is_empty());
    }

    /// Verifies with_store attaches a store to the manager.
    #[test]
    fn with_store_sets_store() {
        let (mgr, _tmp) = manager_with_store();
        assert!(mgr.store.is_some());
    }

    /// Ensures load_conversation_list returns None when no store is set.
    #[test]
    fn load_conversation_list_without_store_returns_none() {
        let mut mgr = ConversationManager::new();
        assert!(mgr.load_conversation_list().is_none());
    }

    /// Ensures load_conversation_list returns empty for a fresh store.
    #[test]
    fn load_conversation_list_empty_store() {
        let (mut mgr, _tmp) = manager_with_store();
        let list = mgr.load_conversation_list().unwrap();
        assert!(list.titles.is_empty());
        assert!(list.ids.is_empty());
    }

    /// Verifies load_conversation_list returns titles and IDs for saved conversations.
    #[test]
    fn load_conversation_list_returns_saved_conversations() {
        let (mut mgr, _tmp) = manager_with_store();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());

        let list = mgr.load_conversation_list().unwrap();
        assert_eq!(list.titles.len(), 2);
        assert_eq!(list.ids.len(), 2);
    }

    /// Ensures create_new_conversation returns an ID and title, sets active, and inserts at front.
    #[test]
    fn create_new_conversation_sets_active() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id, title) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());

        assert_eq!(title, "New Chat");
        assert!(mgr.active_conversation.is_some());
        assert_eq!(mgr.active_conversation.as_ref().unwrap().id, id);
        assert_eq!(mgr.conversation_ids[0], id);
    }

    /// Verifies creating a second conversation saves the first and sets the new one active.
    #[test]
    fn create_new_conversation_saves_previous() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id1, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("hello from first"));
        let (id2, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());

        assert_ne!(id1, id2);
        assert_eq!(mgr.active_conversation.as_ref().unwrap().id, id2);
        // First conversation should be loadable from store
        let loaded = mgr.store.as_ref().unwrap().load(id1).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    /// Ensures switch_to_conversation with invalid index returns an error.
    #[test]
    fn switch_to_conversation_invalid_index() {
        let (mut mgr, _tmp) = manager_with_store();
        let result = mgr.switch_to_conversation(999);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid index"));
    }

    /// Ensures switch_to_conversation without a store returns an error.
    #[test]
    fn switch_to_conversation_no_store() {
        let mut mgr = ConversationManager::new();
        mgr.conversation_ids.push(Uuid::new_v4());
        let result = mgr.switch_to_conversation(0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No store"));
    }

    /// Verifies switch_to_conversation loads the correct conversation.
    #[test]
    fn switch_to_conversation_loads_correct_one() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id1, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("first chat"));

        let (id2, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("second chat"));

        // IDs are [id2, id1] since they're inserted at front
        let conv = mgr.switch_to_conversation(1).unwrap();
        assert_eq!(conv.id, id1);
        assert_eq!(conv.messages[0].content, "first chat");
    }

    /// Ensures delete_conversation with out-of-bounds index returns None.
    #[test]
    fn delete_conversation_out_of_bounds() {
        let (mut mgr, _tmp) = manager_with_store();
        assert!(mgr.delete_conversation(0).is_none());
    }

    /// Verifies deleting the active conversation clears it and returns was_active=true.
    #[test]
    fn delete_active_conversation() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());

        let result = mgr.delete_conversation(0).unwrap();
        assert_eq!(result.0, id);
        assert!(result.1); // was_active
        assert!(mgr.active_conversation.is_none());
        assert!(mgr.conversation_ids.is_empty());
    }

    /// Verifies deleting a non-active conversation doesn't affect the active one.
    #[test]
    fn delete_non_active_conversation() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id1, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        let (_id2, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());

        // Active is id2, delete id1 (index 1 since ids are [id2, id1])
        let result = mgr.delete_conversation(1).unwrap();
        assert_eq!(result.0, id1);
        assert!(!result.1); // not was_active
        assert!(mgr.active_conversation.is_some());
    }

    /// Ensures save_active_conversation is a no-op when there's no active conversation.
    #[test]
    fn save_active_conversation_no_active() {
        let (mgr, _tmp) = manager_with_store();
        mgr.save_active_conversation(); // should not panic
    }

    /// Verifies save_active_conversation persists the current state to the store.
    #[test]
    fn save_active_conversation_persists() {
        let (mut mgr, _tmp) = manager_with_store();
        let (id, _) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("persisted"));
        mgr.save_active_conversation();

        let loaded = mgr.store.as_ref().unwrap().load(id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "persisted");
    }

    /// Ensures update_conversation_title returns None when no active conversation exists.
    #[test]
    fn update_title_no_active() {
        let mut mgr = ConversationManager::new();
        assert!(mgr.update_conversation_title().is_none());
    }

    /// Ensures update_conversation_title returns None when title is already set.
    #[test]
    fn update_title_already_set() {
        let (mut mgr, _tmp) = manager_with_store();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.active_conversation.as_mut().unwrap().title = "Custom Title".to_string();
        mgr.add_message(Message::user("hello"));
        assert!(mgr.update_conversation_title().is_none());
    }

    /// Ensures update_conversation_title returns None when there are no messages.
    #[test]
    fn update_title_no_messages() {
        let (mut mgr, _tmp) = manager_with_store();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        assert!(mgr.update_conversation_title().is_none());
    }

    /// Verifies update_conversation_title auto-titles from the first user message.
    #[test]
    fn update_title_from_first_message() {
        let (mut mgr, _tmp) = manager_with_store();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("Hello world"));

        let result = mgr.update_conversation_title().unwrap();
        assert_eq!(result.0, 0); // index
        assert_eq!(result.1, "Hello world");
    }

    /// Ensures messages() returns empty slice when no active conversation.
    #[test]
    fn messages_no_active() {
        let mgr = ConversationManager::new();
        assert!(mgr.messages().is_empty());
    }

    /// Verifies messages() returns the active conversation's messages.
    #[test]
    fn messages_returns_active_messages() {
        let (mut mgr, _tmp) = manager_with_store();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("msg1"));
        mgr.add_message(Message::assistant("msg2"));

        assert_eq!(mgr.messages().len(), 2);
    }

    /// Ensures add_message is a no-op when there's no active conversation.
    #[test]
    fn add_message_no_active() {
        let mut mgr = ConversationManager::new();
        mgr.add_message(Message::user("orphan")); // should not panic
        assert!(mgr.messages().is_empty());
    }

    /// Verifies create_new_conversation works without a store (no persistence).
    #[test]
    fn create_without_store() {
        let mut mgr = ConversationManager::new();
        let (id, title) = mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        assert_eq!(title, "New Chat");
        assert!(mgr.active_conversation.is_some());
        assert_eq!(mgr.conversation_ids[0], id);
    }

    /// Verifies save_active_conversation is safe without a store.
    #[test]
    fn save_without_store_is_noop() {
        let mut mgr = ConversationManager::new();
        mgr.create_new_conversation("openai".into(), "gpt-4o".into());
        mgr.add_message(Message::user("hello"));
        mgr.save_active_conversation(); // should not panic
    }
}
