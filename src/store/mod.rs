pub mod json_store;
pub mod sqlite_store;
pub mod types;

use uuid::Uuid;

use crate::llm::types::Message;
use types::{Checkpoint, Conversation, ConversationSummary, MessageUsage};

/// Errors from the conversation store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    Io(String),
    Serialize(String),
    Deserialize(String),
    NotFound(String),
    Database(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(msg) => write!(f, "IO error: {msg}"),
            StoreError::Serialize(msg) => write!(f, "Serialize error: {msg}"),
            StoreError::Deserialize(msg) => write!(f, "Deserialize error: {msg}"),
            StoreError::NotFound(msg) => write!(f, "Not found: {msg}"),
            StoreError::Database(msg) => write!(f, "Database error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Persistence trait for conversation storage.
pub trait Store: Send + Sync {
    fn list(&self) -> Result<Vec<ConversationSummary>, StoreError>;
    fn load(&self, id: Uuid) -> Result<Conversation, StoreError>;
    fn save(&self, conversation: &Conversation) -> Result<(), StoreError>;
    fn delete(&self, id: Uuid) -> Result<(), StoreError>;

    /// Atomically append a message and update conversation usage totals.
    fn append_message(
        &self,
        conversation_id: Uuid,
        message: &Message,
        usage: Option<&MessageUsage>,
    ) -> Result<(), StoreError>;

    /// Save a pre-compaction checkpoint of the message history.
    fn save_checkpoint(
        &self,
        conversation_id: Uuid,
        messages: &[Message],
        reason: Option<&str>,
    ) -> Result<(), StoreError>;

    /// Load all checkpoints for a conversation, ordered by creation time.
    fn load_checkpoints(&self, conversation_id: Uuid) -> Result<Vec<Checkpoint>, StoreError>;

    /// Remove oldest checkpoints beyond the max limit.
    fn prune_checkpoints(&self, conversation_id: Uuid, max: usize) -> Result<(), StoreError>;

    /// Load the most recent checkpoint for a conversation.
    fn latest_checkpoint(&self, conversation_id: Uuid) -> Result<Option<Checkpoint>, StoreError> {
        let checkpoints = self.load_checkpoints(conversation_id)?;
        Ok(checkpoints.into_iter().last())
    }

    /// Delete a specific checkpoint by ID.
    fn delete_checkpoint(&self, checkpoint_id: i64) -> Result<(), StoreError>;

    /// Export a conversation as a JSON string.
    fn export_conversation(&self, id: Uuid) -> Result<String, StoreError>;

    /// Import a conversation from a JSON string, assigning a new ID.
    fn import_conversation(&self, json: &str) -> Result<Uuid, StoreError>;
}
