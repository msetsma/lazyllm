# Data Storage and Persistence

lazyllm uses SQLite for all conversation persistence. A single database file stores conversations, messages, and pre-compaction checkpoints.

## Database Location

```
~/.local/share/lazyllm/lazyllm.db
```

Configurable via `general.data_dir` in `config.toml`. The directory is created automatically on first run.

SQLite is configured with:
- **WAL mode** — concurrent reads without blocking writes
- **Foreign keys** — enforced with `ON DELETE CASCADE`

## Schema

### `conversations`

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | UUID string |
| `title` | TEXT | Auto-generated from first user message |
| `model` | TEXT | LLM model ID |
| `provider` | TEXT | Provider name |
| `context_name` | TEXT | Active context file (nullable) |
| `created_at` | TEXT | RFC 3339 timestamp |
| `updated_at` | TEXT | RFC 3339 timestamp |
| `total_input_tokens` | INTEGER | Cumulative prompt tokens |
| `total_output_tokens` | INTEGER | Cumulative completion tokens |
| `total_cache_tokens` | INTEGER | Cumulative cache tokens (v2) |
| `total_cost` | REAL | Cumulative USD cost (v2) |
| `turn_count` | INTEGER | Number of assistant turns (v2) |
| `context_estimate` | INTEGER | Last estimated context size (v2) |

### `messages`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | Auto-increment |
| `conversation_id` | TEXT FK | References `conversations(id)`, CASCADE delete |
| `message_index` | INTEGER | Ordering within conversation |
| `role` | TEXT | `user`, `assistant`, `system`, or `tool` |
| `content` | TEXT | Message body |
| `metadata_json` | TEXT | JSON with `tool_calls` and `tool_call_id` (nullable) |
| `input_tokens` | INTEGER | Prompt tokens for this message (nullable) |
| `output_tokens` | INTEGER | Completion tokens for this message (nullable) |
| `cache_read_tokens` | INTEGER | Anthropic cache reads (v2, nullable) |
| `cache_creation_tokens` | INTEGER | Anthropic cache writes (v2, nullable) |
| `cost` | REAL | USD cost for this turn (v2, nullable) |
| `duration_ms` | INTEGER | Response latency (v2, nullable) |
| `model` | TEXT | Model used for this turn (v2, nullable) |

### `checkpoints`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | Auto-increment |
| `conversation_id` | TEXT FK | References `conversations(id)`, CASCADE delete |
| `snapshot_json` | TEXT | Serialized message history |
| `reason` | TEXT | e.g. `"pre-compaction"` (nullable) |
| `created_at` | TEXT | RFC 3339 timestamp |

### `schema_version`

Single-column table tracking the current migration version.

## Migrations

Migrations run automatically on startup via `SqliteStore::run_migrations()`.

| Version | Changes |
|---------|---------|
| **v1** | Creates `conversations`, `messages`, `checkpoints`, `schema_version` |
| **v2** | Adds token tracking and cost columns to `conversations` and `messages` |

Each migration is idempotent. The version table is append-only — a new row is inserted for each applied migration.

## Store Trait

All persistence goes through the `Store` trait (`src/store/mod.rs`), making the backend swappable:

```rust
pub trait Store: Send + Sync {
    fn list(&self) -> Result<Vec<ConversationSummary>, StoreError>;
    fn load(&self, id: Uuid) -> Result<Conversation, StoreError>;
    fn save(&self, conversation: &Conversation) -> Result<(), StoreError>;
    fn delete(&self, id: Uuid) -> Result<(), StoreError>;

    fn append_message(&self, conversation_id: Uuid, message: &Message, usage: Option<&MessageUsage>) -> Result<(), StoreError>;

    fn save_checkpoint(&self, conversation_id: Uuid, messages: &[Message], reason: Option<&str>) -> Result<(), StoreError>;
    fn load_checkpoints(&self, conversation_id: Uuid) -> Result<Vec<Checkpoint>, StoreError>;
    fn prune_checkpoints(&self, conversation_id: Uuid, max: usize) -> Result<(), StoreError>;

    fn export_conversation(&self, id: Uuid) -> Result<String, StoreError>;
    fn import_conversation(&self, json: &str) -> Result<Uuid, StoreError>;
}
```

### Error Types

| Variant | Meaning |
|---------|---------|
| `Io` | File/directory I/O failure |
| `Serialize` | JSON serialization error |
| `Deserialize` | JSON deserialization error |
| `NotFound` | Conversation UUID not in database |
| `Database` | SQLite error |

## Conversation Lifecycle

### Creation

`ConversationManager::create_new_conversation()` creates a `Conversation` with a new UUID and title `"New Chat"`. It is immediately saved to the store and inserted at the front of the conversation list.

### Auto-Title

After the first user message, `auto_title()` generates a title from the message content (truncated to 40 characters with `...` suffix if needed).

### Saving

- **`save()`** — full upsert: inserts or replaces the conversation row and all message rows in a single transaction
- **`append_message()`** — atomic append: adds a single message and updates the conversation's token/cost totals without rewriting all messages

Messages are appended after each assistant response. Full saves happen on conversation switch and quit.

### Switching

`switch_to_conversation()` saves the current active conversation, then loads the target conversation (with all messages) from the store.

### Deletion

`delete()` removes the conversation row. `CASCADE` foreign keys automatically remove all associated messages and checkpoints.

## Data Types

### `Conversation`

Full conversation state including all messages and cumulative usage. Used when a conversation is actively loaded.

### `ConversationSummary`

Lightweight view for the sidebar list — includes title, model, provider, timestamps, and totals but no message bodies.

### `MessageUsage`

Per-message token and cost data stored alongside messages:

```rust
pub struct MessageUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cost: Option<f64>,
    pub duration_ms: Option<u64>,
    pub model: Option<String>,
}
```

### `Checkpoint`

Serialized snapshot of the full message history taken before compaction. Allows rollback if a compaction summary loses important context.

## Export and Import

- **`export_conversation(id)`** — serializes the full conversation (metadata + messages) as a JSON string
- **`import_conversation(json)`** — deserializes a conversation, assigns a new UUID, and saves it to the store

## Key Files

| File | Purpose |
|------|---------|
| `src/store/mod.rs` | `Store` trait and `StoreError` |
| `src/store/types.rs` | `Conversation`, `ConversationSummary`, `Checkpoint`, `MessageUsage` |
| `src/store/sqlite_store.rs` | SQLite implementation with schema and migrations |
| `src/conversation.rs` | `ConversationManager` — coordinates store with active state |
