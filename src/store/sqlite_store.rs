use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::types::{Message, Role, ToolCall};

use super::types::{Checkpoint, Conversation, ConversationSummary, MessageUsage};
use super::{Store, StoreError};

/// SQLite-backed conversation store.
///
/// Database file: `{data_dir}/lazyllm.db`
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

/// Helper for serializing optional tool_calls / tool_call_id into a single JSON column.
#[derive(Debug, Serialize, Deserialize)]
struct MessageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl SqliteStore {
    pub fn new(data_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        let db_path = data_dir.join("lazyllm.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Return the database file path (for testing).
    #[cfg(test)]
    fn db_path(&self) -> std::path::PathBuf {
        let conn = self.conn.lock().unwrap();
        std::path::PathBuf::from(conn.path().unwrap())
    }

    fn init_schema(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(|e| StoreError::Database(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (
                id                  TEXT PRIMARY KEY,
                title               TEXT NOT NULL,
                model               TEXT NOT NULL,
                provider            TEXT NOT NULL,
                context_name        TEXT,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL,
                total_input_tokens  INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                total_cache_tokens  INTEGER NOT NULL DEFAULT 0,
                total_cost          REAL NOT NULL DEFAULT 0.0,
                turn_count          INTEGER NOT NULL DEFAULT 0,
                context_estimate    INTEGER NOT NULL DEFAULT 0,
                pinned_messages     TEXT NOT NULL DEFAULT '[]',
                session_notes       TEXT,
                compaction_history  TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS messages (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id         TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                message_index           INTEGER NOT NULL,
                role                    TEXT NOT NULL,
                content                 TEXT NOT NULL,
                metadata_json           TEXT,
                input_tokens            INTEGER,
                output_tokens           INTEGER,
                cache_read_tokens       INTEGER,
                cache_creation_tokens   INTEGER,
                cost                    REAL,
                duration_ms             INTEGER,
                model                   TEXT
            );

            CREATE TABLE IF NOT EXISTS checkpoints (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                snapshot_json   TEXT NOT NULL,
                reason          TEXT,
                created_at      TEXT NOT NULL
            );",
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    fn serialize_metadata(msg: &Message) -> Option<String> {
        if msg.tool_calls.is_none() && msg.tool_call_id.is_none() {
            return None;
        }
        let meta = MessageMetadata {
            tool_calls: msg.tool_calls.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        };
        serde_json::to_string(&meta).ok()
    }

    fn deserialize_metadata(json: Option<String>, msg: &mut Message) {
        if let Some(json) = json {
            if let Ok(meta) = serde_json::from_str::<MessageMetadata>(&json) {
                msg.tool_calls = meta.tool_calls;
                msg.tool_call_id = meta.tool_call_id;
            }
        }
    }
}

impl Store for SqliteStore {
    fn list(&self) -> Result<Vec<ConversationSummary>, StoreError> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.title, c.model, c.provider, c.created_at, c.updated_at,
                        (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id),
                        c.total_input_tokens, c.total_output_tokens, c.total_cost, c.turn_count
                 FROM conversations c
                 ORDER BY c.updated_at DESC",
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, usize>(6)?,
                    row.get::<_, u32>(7)?,
                    row.get::<_, u32>(8)?,
                    row.get::<_, f64>(9)?,
                    row.get::<_, u32>(10)?,
                ))
            })
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut summaries = Vec::new();
        for row in rows {
            let (id_str, title, model, provider, created_at_str, updated_at_str,
                 message_count, total_input_tokens, total_output_tokens, total_cost, turn_count) =
                row.map_err(|e| StoreError::Database(e.to_string()))?;

            let id = Uuid::parse_str(&id_str)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?
                .with_timezone(&Utc);
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?
                .with_timezone(&Utc);

            summaries.push(ConversationSummary {
                id,
                title,
                model,
                provider,
                message_count,
                created_at,
                updated_at,
                total_input_tokens,
                total_output_tokens,
                total_cost,
                turn_count,
            });
        }
        Ok(summaries)
    }

    fn load(&self, id: Uuid) -> Result<Conversation, StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.to_string();

        let conv_row = conn
            .query_row(
                "SELECT title, model, provider, context_name, created_at, updated_at,
                        total_input_tokens, total_output_tokens, total_cache_tokens,
                        total_cost, turn_count, context_estimate,
                        pinned_messages, session_notes, compaction_history
                 FROM conversations WHERE id = ?1",
                params![id_str],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, u32>(6)?,
                        row.get::<_, u32>(7)?,
                        row.get::<_, u32>(8)?,
                        row.get::<_, f64>(9)?,
                        row.get::<_, u32>(10)?,
                        row.get::<_, u32>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, Option<String>>(13)?,
                        row.get::<_, String>(14)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::NotFound(format!("Conversation {id} not found"))
                }
                other => StoreError::Database(other.to_string()),
            })?;

        let (title, model, provider, context_name, created_at_str, updated_at_str,
             total_input_tokens, total_output_tokens, total_cache_tokens,
             total_cost, turn_count, context_estimate,
             pinned_json, session_notes, history_json) = conv_row;

        let pinned_messages: Vec<usize> = serde_json::from_str(&pinned_json)
            .unwrap_or_default();
        let compaction_history: Vec<super::types::CompactionEvent> =
            serde_json::from_str(&history_json).unwrap_or_default();

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| StoreError::Deserialize(e.to_string()))?
            .with_timezone(&Utc);
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map_err(|e| StoreError::Deserialize(e.to_string()))?
            .with_timezone(&Utc);

        // Load messages
        let mut stmt = conn
            .prepare(
                "SELECT role, content, metadata_json
                 FROM messages
                 WHERE conversation_id = ?1
                 ORDER BY message_index ASC",
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let msg_rows = stmt
            .query_map(params![id_str], |row| {
                let role_str: String = row.get(0)?;
                let content: String = row.get(1)?;
                let metadata_json: Option<String> = row.get(2)?;
                Ok((role_str, content, metadata_json))
            })
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        for row in msg_rows {
            let (role_str, content, metadata_json) =
                row.map_err(|e| StoreError::Database(e.to_string()))?;

            let role: Role = serde_json::from_value(serde_json::Value::String(role_str))
                .map_err(|e| StoreError::Deserialize(e.to_string()))?;

            let mut msg = Message {
                role,
                content,
                tool_calls: None,
                tool_call_id: None,
            };
            Self::deserialize_metadata(metadata_json, &mut msg);
            messages.push(msg);
        }

        Ok(Conversation {
            id,
            title,
            messages,
            model,
            provider,
            created_at,
            updated_at,
            context_name,
            total_input_tokens,
            total_output_tokens,
            total_cache_tokens,
            total_cost,
            turn_count,
            context_estimate,
            pinned_messages,
            session_notes,
            compaction_history,
        })
    }

    fn save(&self, conversation: &Conversation) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let id_str = conversation.id.to_string();
        let created_at = conversation.created_at.to_rfc3339();
        let updated_at = conversation.updated_at.to_rfc3339();

        let tx = conn
            .transaction()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let pinned_json = serde_json::to_string(&conversation.pinned_messages)
            .unwrap_or_else(|_| "[]".to_string());
        let history_json = serde_json::to_string(&conversation.compaction_history)
            .unwrap_or_else(|_| "[]".to_string());

        // Upsert conversation row
        tx.execute(
            "INSERT INTO conversations (id, title, model, provider, context_name, created_at, updated_at,
                total_input_tokens, total_output_tokens, total_cache_tokens, total_cost, turn_count, context_estimate,
                pinned_messages, session_notes, compaction_history)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                model = excluded.model,
                provider = excluded.provider,
                context_name = excluded.context_name,
                updated_at = excluded.updated_at,
                total_input_tokens = excluded.total_input_tokens,
                total_output_tokens = excluded.total_output_tokens,
                total_cache_tokens = excluded.total_cache_tokens,
                total_cost = excluded.total_cost,
                turn_count = excluded.turn_count,
                context_estimate = excluded.context_estimate,
                pinned_messages = excluded.pinned_messages,
                session_notes = excluded.session_notes,
                compaction_history = excluded.compaction_history",
            params![
                id_str,
                conversation.title,
                conversation.model,
                conversation.provider,
                conversation.context_name,
                created_at,
                updated_at,
                conversation.total_input_tokens,
                conversation.total_output_tokens,
                conversation.total_cache_tokens,
                conversation.total_cost,
                conversation.turn_count,
                conversation.context_estimate,
                pinned_json,
                conversation.session_notes,
                history_json,
            ],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        // Delete old messages and insert new ones
        tx.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![id_str],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        for (idx, msg) in conversation.messages.iter().enumerate() {
            let metadata_json = Self::serialize_metadata(msg);
            tx.execute(
                "INSERT INTO messages (conversation_id, message_index, role, content, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id_str,
                    idx as i64,
                    msg.role.as_str(),
                    msg.content,
                    metadata_json,
                ],
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.to_string();
        conn.execute("DELETE FROM conversations WHERE id = ?1", params![id_str])
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn append_message(
        &self,
        conversation_id: Uuid,
        message: &Message,
        usage: Option<&MessageUsage>,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let id_str = conversation_id.to_string();
        let now = Utc::now().to_rfc3339();

        let tx = conn
            .transaction()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        // Get the next message index
        let next_index: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(message_index), -1) + 1 FROM messages WHERE conversation_id = ?1",
                params![id_str],
                |row| row.get(0),
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let metadata_json = Self::serialize_metadata(message);
        let (input_tokens, output_tokens, cache_read, cache_creation, cost, duration_ms, model) =
            match usage {
                Some(u) => (
                    u.input_tokens, u.output_tokens, u.cache_read_tokens,
                    u.cache_creation_tokens, u.cost, u.duration_ms,
                    u.model.clone(),
                ),
                None => (None, None, None, None, None, None, None),
            };

        tx.execute(
            "INSERT INTO messages (conversation_id, message_index, role, content, metadata_json,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                cost, duration_ms, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id_str, next_index, message.role.as_str(), message.content,
                metadata_json, input_tokens, output_tokens, cache_read,
                cache_creation, cost, duration_ms, model,
            ],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        // Update conversation totals and timestamp
        let (add_input, add_output, add_cache, add_cost) = match usage {
            Some(u) => (
                u.input_tokens.unwrap_or(0) as i64,
                u.output_tokens.unwrap_or(0) as i64,
                u.cache_read_tokens.unwrap_or(0) as i64
                    + u.cache_creation_tokens.unwrap_or(0) as i64,
                u.cost.unwrap_or(0.0),
            ),
            None => (0, 0, 0, 0.0),
        };

        tx.execute(
            "UPDATE conversations SET
                updated_at = ?2,
                total_input_tokens = total_input_tokens + ?3,
                total_output_tokens = total_output_tokens + ?4,
                total_cache_tokens = total_cache_tokens + ?5,
                total_cost = total_cost + ?6,
                turn_count = turn_count + 1
             WHERE id = ?1",
            params![id_str, now, add_input, add_output, add_cache, add_cost],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn save_checkpoint(
        &self,
        conversation_id: Uuid,
        messages: &[Message],
        reason: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = conversation_id.to_string();
        let snapshot = serde_json::to_string(messages)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO checkpoints (conversation_id, snapshot_json, reason, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id_str, snapshot, reason, now],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn load_checkpoints(&self, conversation_id: Uuid) -> Result<Vec<Checkpoint>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = conversation_id.to_string();

        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, snapshot_json, reason, created_at
                 FROM checkpoints
                 WHERE conversation_id = ?1
                 ORDER BY created_at ASC",
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![id_str], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut checkpoints = Vec::new();
        for row in rows {
            let (id, conv_id_str, snapshot_json, reason, created_at_str) =
                row.map_err(|e| StoreError::Database(e.to_string()))?;
            let conv_id = Uuid::parse_str(&conv_id_str)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?
                .with_timezone(&Utc);
            checkpoints.push(Checkpoint {
                id,
                conversation_id: conv_id,
                snapshot_json,
                reason,
                created_at,
            });
        }
        Ok(checkpoints)
    }

    fn prune_checkpoints(&self, conversation_id: Uuid, max: usize) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = conversation_id.to_string();

        conn.execute(
            "DELETE FROM checkpoints WHERE id IN (
                SELECT id FROM checkpoints
                WHERE conversation_id = ?1
                ORDER BY created_at DESC
                LIMIT -1 OFFSET ?2
             )",
            params![id_str, max as i64],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn delete_checkpoint(&self, checkpoint_id: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM checkpoints WHERE id = ?1",
            params![checkpoint_id],
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (SqliteStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new(tmp.path()).unwrap();
        (store, tmp)
    }

    fn sample_conversation() -> Conversation {
        Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Test Chat")
            .add_message(Message::user("hello"))
            .add_message(Message::assistant("hi there"))
    }

    /// Ensures the SQLite database file is created on store initialization.
    #[test]
    fn new_creates_database_file() {
        let (store, _tmp) = test_store();
        assert!(store.db_path().exists());
    }

    /// Verifies conversations survive SQLite save/load roundtrip with all fields intact.
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
        for (a, b) in loaded.messages.iter().zip(conv.messages.iter()) {
            assert_eq!(a.role, b.role);
            assert_eq!(a.content, b.content);
        }
    }

    /// Ensures saving the same conversation ID replaces messages with updated content.
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

    /// Ensures loading a non-existent conversation returns an error.
    #[test]
    fn load_nonexistent_returns_error() {
        let (store, _tmp) = test_store();
        let result = store.load(Uuid::new_v4());
        assert!(result.is_err());
    }

    /// Verifies delete removes the conversation from the database.
    #[test]
    fn delete_removes_conversation() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        let id = conv.id;

        store.save(&conv).unwrap();
        store.delete(id).unwrap();
        assert!(store.load(id).is_err());
    }

    /// Ensures deleting a non-existent conversation is a no-op.
    #[test]
    fn delete_nonexistent_is_ok() {
        let (store, _tmp) = test_store();
        let result = store.delete(Uuid::new_v4());
        assert!(result.is_ok());
    }

    /// Verifies list() returns summaries for all saved conversations.
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

    /// Ensures list() returns empty vec for a fresh database.
    #[test]
    fn list_empty_store() {
        let (store, _tmp) = test_store();
        let summaries = store.list().unwrap();
        assert!(summaries.is_empty());
    }

    /// Verifies list() returns conversations sorted by updated_at descending.
    #[test]
    fn list_sorted_by_updated_at_desc() {
        let (store, _tmp) = test_store();

        let conv1 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Older");
        store.save(&conv1).unwrap();

        let conv2 = Conversation::new("openai".to_string(), "gpt-4o".to_string())
            .with_title("Newer")
            .add_message(Message::user("hi"));
        store.save(&conv2).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 2);
        assert!(summaries[0].updated_at >= summaries[1].updated_at);
    }

    /// Verifies the summary message_count matches actual stored messages.
    #[test]
    fn summary_has_correct_message_count() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries[0].message_count, 2);
    }

    /// Ensures tool_use and tool_result messages with metadata survive SQLite roundtrip.
    #[test]
    fn tool_calls_roundtrip() {
        let (store, _tmp) = test_store();

        let tool_calls = vec![ToolCall {
            id: "tc_1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp/test"}"#.to_string(),
        }];

        let mut conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        conv.messages.push(Message::user("read a file"));
        conv.messages.push(Message::tool_use(tool_calls.clone()));
        conv.messages
            .push(Message::tool_result("tc_1", "file contents"));
        conv.messages
            .push(Message::assistant("Here are the contents"));

        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();

        assert_eq!(loaded.messages.len(), 4);

        let tool_msg = &loaded.messages[1];
        assert_eq!(tool_msg.role, Role::Assistant);
        let tc = tool_msg.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "tc_1");
        assert_eq!(tc[0].name, "read_file");

        let result_msg = &loaded.messages[2];
        assert_eq!(result_msg.role, Role::Tool);
        assert_eq!(result_msg.tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(result_msg.content, "file contents");
    }

    /// Verifies the optional context_name field persists through SQLite roundtrip.
    #[test]
    fn context_name_roundtrip() {
        let (store, _tmp) = test_store();

        let mut conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        conv.context_name = Some("coding".to_string());

        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.context_name, Some("coding".to_string()));
    }

    /// Ensures running init_schema multiple times is safe and does not corrupt data.
    #[test]
    fn schema_init_is_idempotent() {
        let (store, _tmp) = test_store();

        store.init_schema().unwrap();

        let conv = sample_conversation();
        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.title, conv.title);
    }

    /// Verifies token usage and cost fields persist through SQLite roundtrip.
    #[test]
    fn usage_fields_roundtrip() {
        let (store, _tmp) = test_store();
        let mut conv = sample_conversation();
        conv.total_input_tokens = 100;
        conv.total_output_tokens = 200;
        conv.total_cache_tokens = 50;
        conv.total_cost = 0.0035;
        conv.turn_count = 3;
        conv.context_estimate = 4096;

        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();

        assert_eq!(loaded.total_input_tokens, 100);
        assert_eq!(loaded.total_output_tokens, 200);
        assert_eq!(loaded.total_cache_tokens, 50);
        assert!((loaded.total_cost - 0.0035).abs() < f64::EPSILON);
        assert_eq!(loaded.turn_count, 3);
        assert_eq!(loaded.context_estimate, 4096);
    }

    /// Ensures usage fields are included in list() summaries.
    #[test]
    fn list_includes_usage_fields() {
        let (store, _tmp) = test_store();
        let mut conv = sample_conversation();
        conv.total_input_tokens = 42;
        conv.total_cost = 0.01;
        conv.turn_count = 5;
        store.save(&conv).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries[0].total_input_tokens, 42);
        assert!((summaries[0].total_cost - 0.01).abs() < f64::EPSILON);
        assert_eq!(summaries[0].turn_count, 5);
    }

    /// Verifies append_message adds a message and atomically updates usage totals.
    #[test]
    fn append_message_adds_message_and_updates_totals() {
        let (store, _tmp) = test_store();
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        store.save(&conv).unwrap();

        let usage = MessageUsage {
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_tokens: Some(5),
            cost: Some(0.001),
            ..Default::default()
        };
        store
            .append_message(conv.id, &Message::user("hello"), Some(&usage))
            .unwrap();

        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "hello");
        assert_eq!(loaded.total_input_tokens, 10);
        assert_eq!(loaded.total_output_tokens, 20);
        assert_eq!(loaded.total_cache_tokens, 5);
        assert!((loaded.total_cost - 0.001).abs() < f64::EPSILON);
        assert_eq!(loaded.turn_count, 1);
    }

    /// Ensures append_message works without usage data (totals stay at zero).
    #[test]
    fn append_message_without_usage() {
        let (store, _tmp) = test_store();
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        store.save(&conv).unwrap();

        store
            .append_message(conv.id, &Message::user("hello"), None)
            .unwrap();

        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.total_input_tokens, 0);
        assert_eq!(loaded.turn_count, 1);
    }

    /// Verifies multiple append_message calls accumulate token totals correctly.
    #[test]
    fn append_message_accumulates_totals() {
        let (store, _tmp) = test_store();
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        store.save(&conv).unwrap();

        let usage1 = MessageUsage {
            input_tokens: Some(10),
            output_tokens: Some(20),
            ..Default::default()
        };
        let usage2 = MessageUsage {
            input_tokens: Some(15),
            output_tokens: Some(30),
            ..Default::default()
        };

        store.append_message(conv.id, &Message::user("hi"), Some(&usage1)).unwrap();
        store.append_message(conv.id, &Message::assistant("hello"), Some(&usage2)).unwrap();

        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.total_input_tokens, 25);
        assert_eq!(loaded.total_output_tokens, 50);
        assert_eq!(loaded.turn_count, 2);
    }

    /// Ensures tool_calls metadata is preserved when appending via append_message.
    #[test]
    fn append_message_preserves_tool_metadata() {
        let (store, _tmp) = test_store();
        let conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        store.save(&conv).unwrap();

        let tool_calls = vec![ToolCall {
            id: "tc_1".to_string(),
            name: "test".to_string(),
            arguments: "{}".to_string(),
        }];
        store
            .append_message(conv.id, &Message::tool_use(tool_calls), None)
            .unwrap();

        let loaded = store.load(conv.id).unwrap();
        let tc = loaded.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].id, "tc_1");
    }

    /// Verifies checkpoints can be saved and loaded with reason and message snapshot.
    #[test]
    fn checkpoint_save_and_load() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        store
            .save_checkpoint(conv.id, &conv.messages, Some("auto compaction"))
            .unwrap();

        let checkpoints = store.load_checkpoints(conv.id).unwrap();
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].conversation_id, conv.id);
        assert_eq!(checkpoints[0].reason.as_deref(), Some("auto compaction"));

        let restored: Vec<Message> =
            serde_json::from_str(&checkpoints[0].snapshot_json).unwrap();
        assert_eq!(restored.len(), 2);
    }

    /// Ensures loading checkpoints for a non-existent conversation returns empty.
    #[test]
    fn checkpoint_load_empty() {
        let (store, _tmp) = test_store();
        let checkpoints = store.load_checkpoints(Uuid::new_v4()).unwrap();
        assert!(checkpoints.is_empty());
    }

    /// Verifies prune_checkpoints keeps only the N newest checkpoints.
    #[test]
    fn prune_checkpoints_keeps_newest() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        for i in 0..5 {
            store
                .save_checkpoint(conv.id, &conv.messages, Some(&format!("cp {i}")))
                .unwrap();
        }

        store.prune_checkpoints(conv.id, 2).unwrap();
        let checkpoints = store.load_checkpoints(conv.id).unwrap();
        assert_eq!(checkpoints.len(), 2);
        assert_eq!(checkpoints[0].reason.as_deref(), Some("cp 3"));
        assert_eq!(checkpoints[1].reason.as_deref(), Some("cp 4"));
    }

    /// Ensures checkpoints are cascade-deleted when the parent conversation is deleted.
    #[test]
    fn checkpoint_deleted_with_conversation() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();
        store
            .save_checkpoint(conv.id, &conv.messages, None)
            .unwrap();

        store.delete(conv.id).unwrap();
        let checkpoints = store.load_checkpoints(conv.id).unwrap();
        assert!(checkpoints.is_empty());
    }

    /// Verifies export/import creates a new conversation with a different ID but same content.
    #[test]
    fn export_and_import_roundtrip() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        store.save(&conv).unwrap();

        let json = store.export_conversation(conv.id).unwrap();
        let new_id = store.import_conversation(&json).unwrap();

        assert_ne!(new_id, conv.id);

        let imported = store.load(new_id).unwrap();
        assert_eq!(imported.title, conv.title);
        assert_eq!(imported.messages.len(), conv.messages.len());
        assert_eq!(imported.model, conv.model);
    }

    /// Ensures exporting a non-existent conversation returns an error.
    #[test]
    fn export_nonexistent_returns_error() {
        let (store, _tmp) = test_store();
        assert!(store.export_conversation(Uuid::new_v4()).is_err());
    }

    /// Ensures importing invalid JSON returns an error.
    #[test]
    fn import_invalid_json_returns_error() {
        let (store, _tmp) = test_store();
        assert!(store.import_conversation("not json").is_err());
    }
}
