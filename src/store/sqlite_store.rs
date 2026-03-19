use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::types::{Message, Role, ToolCall};

use super::types::{Conversation, ConversationSummary};
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
        store.run_migrations()?;
        Ok(store)
    }

    /// Return the database file path (for testing).
    #[cfg(test)]
    fn db_path(&self) -> std::path::PathBuf {
        let conn = self.conn.lock().unwrap();
        std::path::PathBuf::from(conn.path().unwrap())
    }

    fn run_migrations(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(|e| StoreError::Database(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL
            );",
        )
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        if version < 1 {
            Self::migrate_v1(&conn)?;
        }

        Ok(())
    }

    fn migrate_v1(conn: &Connection) -> Result<(), StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (
                id              TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                model           TEXT NOT NULL,
                provider        TEXT NOT NULL,
                context_name    TEXT,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                total_input_tokens  INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS messages (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                message_index   INTEGER NOT NULL,
                role            TEXT NOT NULL,
                content         TEXT NOT NULL,
                metadata_json   TEXT,
                input_tokens    INTEGER,
                output_tokens   INTEGER
            );

            CREATE TABLE IF NOT EXISTS checkpoints (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                snapshot_json   TEXT NOT NULL,
                reason          TEXT,
                created_at      TEXT NOT NULL
            );

            INSERT INTO schema_version (version) VALUES (1);",
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
                        (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)
                 FROM conversations c
                 ORDER BY c.updated_at DESC",
            )
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let title: String = row.get(1)?;
                let model: String = row.get(2)?;
                let provider: String = row.get(3)?;
                let created_at_str: String = row.get(4)?;
                let updated_at_str: String = row.get(5)?;
                let message_count: usize = row.get(6)?;
                Ok((id_str, title, model, provider, created_at_str, updated_at_str, message_count))
            })
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut summaries = Vec::new();
        for row in rows {
            let (id_str, title, model, provider, created_at_str, updated_at_str, message_count) =
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
            });
        }
        Ok(summaries)
    }

    fn load(&self, id: Uuid) -> Result<Conversation, StoreError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.to_string();

        let (title, model, provider, context_name, created_at_str, updated_at_str): (
            String, String, String, Option<String>, String, String,
        ) = conn
            .query_row(
                "SELECT title, model, provider, context_name, created_at, updated_at
                 FROM conversations WHERE id = ?1",
                params![id_str],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::NotFound(format!("Conversation {id} not found"))
                }
                other => StoreError::Database(other.to_string()),
            })?;

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

        // Upsert conversation row
        tx.execute(
            "INSERT INTO conversations (id, title, model, provider, context_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                model = excluded.model,
                provider = excluded.provider,
                context_name = excluded.context_name,
                updated_at = excluded.updated_at",
            params![
                id_str,
                conversation.title,
                conversation.model,
                conversation.provider,
                conversation.context_name,
                created_at,
                updated_at,
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

    #[test]
    fn new_creates_database_file() {
        let (store, _tmp) = test_store();
        assert!(store.db_path().exists());
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
        for (a, b) in loaded.messages.iter().zip(conv.messages.iter()) {
            assert_eq!(a.role, b.role);
            assert_eq!(a.content, b.content);
        }
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
    fn delete_removes_conversation() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation();
        let id = conv.id;

        store.save(&conv).unwrap();
        store.delete(id).unwrap();
        assert!(store.load(id).is_err());
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
    fn list_empty_store() {
        let (store, _tmp) = test_store();
        let summaries = store.list().unwrap();
        assert!(summaries.is_empty());
    }

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

    #[test]
    fn summary_has_correct_message_count() {
        let (store, _tmp) = test_store();
        let conv = sample_conversation(); // 2 messages
        store.save(&conv).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries[0].message_count, 2);
    }

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

        // Check tool_use message
        let tool_msg = &loaded.messages[1];
        assert_eq!(tool_msg.role, Role::Assistant);
        let tc = tool_msg.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "tc_1");
        assert_eq!(tc[0].name, "read_file");

        // Check tool_result message
        let result_msg = &loaded.messages[2];
        assert_eq!(result_msg.role, Role::Tool);
        assert_eq!(result_msg.tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(result_msg.content, "file contents");
    }

    #[test]
    fn context_name_roundtrip() {
        let (store, _tmp) = test_store();

        let mut conv = Conversation::new("openai".to_string(), "gpt-4o".to_string());
        conv.context_name = Some("coding".to_string());

        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.context_name, Some("coding".to_string()));
    }

    #[test]
    fn migration_is_idempotent() {
        let (store, _tmp) = test_store();

        // Run migrations again — should not fail
        store.run_migrations().unwrap();

        // Store should still work
        let conv = sample_conversation();
        store.save(&conv).unwrap();
        let loaded = store.load(conv.id).unwrap();
        assert_eq!(loaded.title, conv.title);
    }
}
