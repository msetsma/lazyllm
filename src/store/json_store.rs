use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::llm::types::Message;
use super::types::{Checkpoint, Conversation, ConversationSummary, MessageUsage};
use super::{Store, StoreError};

/// File-based conversation store using JSON files.
///
/// Each conversation is stored as a separate JSON file in the data directory:
/// `{data_dir}/conversations/{uuid}.json`
#[derive(Debug, Clone)]
pub struct JsonStore {
    conversations_dir: PathBuf,
}

impl JsonStore {
    pub fn new(data_dir: &Path) -> std::io::Result<Self> {
        let conversations_dir = data_dir.join("conversations");
        std::fs::create_dir_all(&conversations_dir)?;
        Ok(Self { conversations_dir })
    }

    fn conversation_path(&self, id: Uuid) -> PathBuf {
        self.conversations_dir.join(format!("{id}.json"))
    }

    fn load_from_path(&self, path: &Path) -> Result<Conversation, StoreError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let conversation: Conversation = serde_json::from_str(&contents)
            .map_err(|e| StoreError::Deserialize(e.to_string()))?;
        Ok(conversation)
    }
}

impl Store for JsonStore {
    /// List all conversation summaries, sorted by updated_at descending.
    fn list(&self) -> Result<Vec<ConversationSummary>, StoreError> {
        let mut summaries = Vec::new();

        let entries = std::fs::read_dir(&self.conversations_dir)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        for entry in entries {
            let entry = entry.map_err(|e| StoreError::Io(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match self.load_from_path(&path) {
                Ok(conv) => summaries.push(conv.summary()),
                Err(e) => {
                    tracing::warn!("Failed to load conversation {:?}: {}", path, e);
                    continue;
                }
            }
        }

        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    /// Load a full conversation by ID.
    fn load(&self, id: Uuid) -> Result<Conversation, StoreError> {
        let path = self.conversation_path(id);
        self.load_from_path(&path)
    }

    /// Save a conversation (creates or overwrites).
    fn save(&self, conversation: &Conversation) -> Result<(), StoreError> {
        let path = self.conversation_path(conversation.id);
        let json = serde_json::to_string_pretty(conversation)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        std::fs::write(&path, json)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    /// Delete a conversation by ID.
    fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let path = self.conversation_path(id);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| StoreError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn append_message(
        &self,
        conversation_id: Uuid,
        message: &Message,
        usage: Option<&MessageUsage>,
    ) -> Result<(), StoreError> {
        let mut conversation = self.load(conversation_id)?;
        conversation.messages.push(message.clone());
        conversation.updated_at = Utc::now();
        if let Some(u) = usage {
            conversation.total_input_tokens += u.input_tokens.unwrap_or(0);
            conversation.total_output_tokens += u.output_tokens.unwrap_or(0);
            conversation.total_cache_tokens +=
                u.cache_read_tokens.unwrap_or(0) + u.cache_creation_tokens.unwrap_or(0);
            conversation.total_cost += u.cost.unwrap_or(0.0);
        }
        conversation.turn_count += 1;
        self.save(&conversation)
    }

    fn save_checkpoint(
        &self,
        conversation_id: Uuid,
        messages: &[Message],
        reason: Option<&str>,
    ) -> Result<(), StoreError> {
        let dir = self.conversations_dir.parent()
            .unwrap_or(&self.conversations_dir)
            .join("checkpoints");
        std::fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;

        let path = dir.join(format!("{conversation_id}.json"));
        let mut checkpoints: Vec<Checkpoint> = if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .map_err(|e| StoreError::Io(e.to_string()))?;
            serde_json::from_str(&contents).unwrap_or_default()
        } else {
            Vec::new()
        };

        let snapshot = serde_json::to_string(messages)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        let next_id = checkpoints.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        checkpoints.push(Checkpoint {
            id: next_id,
            conversation_id,
            snapshot_json: snapshot,
            reason: reason.map(|s| s.to_string()),
            created_at: Utc::now(),
        });

        let json = serde_json::to_string_pretty(&checkpoints)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        std::fs::write(&path, json).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    fn load_checkpoints(&self, conversation_id: Uuid) -> Result<Vec<Checkpoint>, StoreError> {
        let path = self.conversations_dir.parent()
            .unwrap_or(&self.conversations_dir)
            .join("checkpoints")
            .join(format!("{conversation_id}.json"));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        serde_json::from_str(&contents)
            .map_err(|e| StoreError::Deserialize(e.to_string()))
    }

    fn prune_checkpoints(&self, conversation_id: Uuid, max: usize) -> Result<(), StoreError> {
        let path = self.conversations_dir.parent()
            .unwrap_or(&self.conversations_dir)
            .join("checkpoints")
            .join(format!("{conversation_id}.json"));
        if !path.exists() {
            return Ok(());
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let mut checkpoints: Vec<Checkpoint> = serde_json::from_str(&contents)
            .map_err(|e| StoreError::Deserialize(e.to_string()))?;
        if checkpoints.len() > max {
            checkpoints.drain(..checkpoints.len() - max);
        }
        let json = serde_json::to_string_pretty(&checkpoints)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        std::fs::write(&path, json).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    fn export_conversation(&self, id: Uuid) -> Result<String, StoreError> {
        let conversation = self.load(id)?;
        serde_json::to_string_pretty(&conversation)
            .map_err(|e| StoreError::Serialize(e.to_string()))
    }

    fn import_conversation(&self, json: &str) -> Result<Uuid, StoreError> {
        let mut conversation: Conversation = serde_json::from_str(json)
            .map_err(|e| StoreError::Deserialize(e.to_string()))?;
        conversation.id = Uuid::new_v4();
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(conversation.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Message;
    use tempfile::TempDir;

    fn test_store() -> (JsonStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = JsonStore::new(tmp.path()).unwrap();
        (store, tmp)
    }

    fn sample_conversation() -> Conversation {
        Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Test Chat")
            .add_message(Message::user("hello"))
            .add_message(Message::assistant("hi there"))
    }

    #[test]
    fn new_creates_conversations_directory() {
        let tmp = TempDir::new().unwrap();
        let _store = JsonStore::new(tmp.path()).unwrap();
        assert!(tmp.path().join("conversations").exists());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        let id = conv.id;

        store.save(&conv).unwrap();
        let loaded = store.load(id).unwrap();

        assert_eq!(loaded.id, conv.id);
        assert_eq!(loaded.title, conv.title);
        assert_eq!(loaded.messages.len(), conv.messages.len());
        assert_eq!(loaded.model, conv.model);
        assert_eq!(loaded.provider, conv.provider);
    }

    #[test]
    fn save_creates_json_file() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        let path = store.conversation_path(conv.id);

        store.save(&conv).unwrap();
        assert!(path.exists());
        assert!(path.extension().unwrap() == "json");
    }

    #[test]
    fn save_overwrites_existing() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        let updated = conv.add_message(Message::user("another message"));
        store.save(&updated).unwrap();

        let loaded = store.load(updated.id).unwrap();
        assert_eq!(loaded.messages.len(), 3);
    }

    #[test]
    fn load_nonexistent_returns_error() {
        let (store, _tmp) = test_store();
        let result = store.load(Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn delete_removes_file() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        let id = conv.id;
        let path = store.conversation_path(id);

        store.save(&conv).unwrap();
        assert!(path.exists());

        store.delete(id).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let (store, _tmp) = test_store();
        let result = store.delete(Uuid::new_v4());
        assert!(result.is_ok());
    }

    #[test]
    fn list_returns_all_conversations() {
        let (store, _tmp) = test_store();

        let conv1 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("First");
        let conv2 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Second");

        store.save(&conv1).unwrap();
        store.save(&conv2).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn list_empty_store_returns_empty() {
        let (store, _tmp) = test_store();
        let summaries = store.list().unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn list_sorted_by_updated_at_descending() {
        let (store, _tmp) = test_store();

        let conv1 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Older");
        store.save(&conv1).unwrap();

        // Create second conversation slightly later
        let conv2 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Newer")
            .add_message(Message::user("hi")); // updates timestamp

        store.save(&conv2).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 2);
        // Newer should be first
        assert!(summaries[0].updated_at >= summaries[1].updated_at);
    }

    #[test]
    fn list_ignores_non_json_files() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        // Create a non-json file
        let bad_file = store.conversations_dir.join("notes.txt");
        std::fs::write(&bad_file, "not a conversation").unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
    }

    #[test]
    fn list_skips_corrupt_files() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        // Create a corrupt json file
        let corrupt = store.conversations_dir.join("bad.json");
        std::fs::write(&corrupt, "{invalid json}").unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1); // Only the valid one
    }

    #[test]
    fn summary_has_correct_message_count() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation(); // 2 messages
        store.save(&conv).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries[0].message_count, 2);
    }

    #[test]
    fn store_error_display() {
        let err = StoreError::Io("permission denied".to_string());
        assert!(err.to_string().contains("permission denied"));

        let err = StoreError::NotFound("uuid-123".to_string());
        assert!(err.to_string().contains("uuid-123"));
    }
}
