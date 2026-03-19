pub mod json_store;
pub mod sqlite_store;
pub mod types;

use uuid::Uuid;

use types::{Conversation, ConversationSummary};

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
}
