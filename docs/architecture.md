# Architecture

## Overview

lazyllm is a terminal-based chat client built with Rust, [ratatui](https://ratatui.rs) for the TUI, and [tokio](https://tokio.rs) for async I/O. The application follows an event-driven architecture with modal input handling inspired by vim.

### Startup Flow

```
main.rs
  ├── Load config (TOML)
  ├── Build ProviderRegistry from config
  ├── Initialize SqliteStore
  ├── Create App (state container)
  ├── Spawn MCP servers
  ├── Setup terminal (raw mode, alternate screen)
  └── Enter event loop
```

### Event Loop

```
Terminal Event → AppEvent → resolve_key() → Action → app.update() → render()
```

1. **`spawn_event_loop()`** polls terminal events (key presses, resize) and ticks at ~30fps
2. **`resolve_key()`** maps key events to `Action` variants based on the current `Mode` and `FocusTarget`
3. **`app.update(action)`** dispatches the action, mutating state (e.g., `SendMessage`, `NewChat`, `SwitchMode`)
4. **`render()`** draws the current state to the terminal

### Core State

The `App` struct (`src/app.rs`) holds all application state:

- **`ProviderRegistry`** — registered LLM providers
- **`ConversationManager`** — active conversation, store, conversation list
- **`McpManager`** — MCP server connections and tool routing
- **Mode/Focus** — current input mode and focused panel
- **Streaming state** — receiver channel for in-progress responses
- **Usage tracking** — last turn and session-level `TokenUsage`
- **Contexts** — loaded context files

## Provider System

### LlmProvider Trait

All providers implement:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn available_models(&self) -> Vec<ModelInfo>;
    async fn chat(&self, request: ChatRequest, tx: mpsc::UnboundedSender<StreamChunk>) -> Result<(), LlmError>;
}
```

The `chat()` method streams the response — the provider sends `StreamChunk` variants through the channel as data arrives from the API.

### Provider Registry

`build_registry()` reads `AppConfig` and instantiates providers:

| `provider_type` | Implementation | Auth | Default Base URL |
|-----------------|----------------|------|------------------|
| `"openai"` | `OpenAiProvider` | API key env var | `https://api.openai.com/v1` |
| `"anthropic"` | `AnthropicProvider` | API key env var | `https://api.anthropic.com/v1/messages` |
| `"google"` | `GoogleProvider` | API key env var | `https://generativelanguage.googleapis.com/v1beta` |
| `"ollama"` | `OllamaProvider` | None required | `http://localhost:11434` |

- Unknown `provider_type` values fall back to OpenAI-compatible
- Providers whose API key env var is not set are silently skipped (logged at WARN)
- Ollama never requires an API key

The OpenAI provider works with any OpenAI-compatible API (Azure, Together AI, local servers like LM Studio) by setting a custom `base_url`.

### ChatRequest

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<ToolDefinition>>,
}
```

### Message Types

```rust
pub enum Role { User, Assistant, System, Tool }

pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,    // assistant requesting tool use
    pub tool_call_id: Option<String>,          // tool result reference
}
```

## Streaming

All providers stream responses via Server-Sent Events (SSE). The streaming module (`src/llm/streaming.rs`) provides shared parsing utilities:

- **`stream_sse_response()`** — parses SSE lines one chunk at a time, calling a provider-specific parse function per line
- **`stream_sse_response_multi()`** — variant that allows multiple `StreamChunk` values per line (used by OpenAI for tool call deltas)
- **`check_http_error()`** — extracts error messages from non-200 responses with provider-specific body parsing

### StreamChunk

```rust
pub enum StreamChunk {
    Delta(String),                                          // text fragment
    Usage(TokenUsage),                                      // token/cost data
    Done,                                                   // stream complete
    Error(String),                                          // error message
    ToolCallStart { id: String, name: String, arguments: String },
    ToolCallResult { id: String, content: String, is_error: bool },
}
```

### Provider-Specific Parsing

Each provider has its own SSE format:

| Provider | Format | Notable Differences |
|----------|--------|---------------------|
| OpenAI | `data: {json}` lines | Tool call deltas accumulated across chunks |
| Anthropic | `event:` + `data:` pairs | Separate `content_block_delta`, `message_delta`, `message_start` events; split usage across events |
| Google | SSE with JSON arrays | Content in `candidates[0].content.parts[0].text` |
| Ollama | Line-delimited JSON | Non-SSE; `message.content` field, `done: true` terminator |

## MCP Integration

The [Model Context Protocol](https://modelcontextprotocol.io) allows lazyllm to use external tools via local MCP servers.

### Architecture

```
App
 └── McpManager
      ├── McpClient ("filesystem")  →  spawned subprocess
      └── McpClient ("custom")      →  spawned subprocess
```

### Lifecycle

1. **Startup**: `McpManager::new()` spawns all configured servers in parallel via `McpClient::spawn()`
2. **Discovery**: each client calls `list_tools()` to discover available tools
3. **Routing**: tool names are mapped to their server via `tool_routing: HashMap<String, String>`
4. **Execution**: when the LLM requests a tool call, `call_tool()` routes to the correct client
5. **Shutdown**: `shutdown_mcp()` sends shutdown signals to all servers

### Configuration

```toml
[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[[mcp.servers]]
name = "custom"
command = "/usr/bin/my-server"
transport = "sse"
url = "http://localhost:8080/sse"
```

Each server entry supports:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Display name and routing key |
| `command` | Yes | Executable to spawn |
| `args` | No | Command-line arguments |
| `env` | No | Environment variables (key-value map) |
| `transport` | No | `"stdio"` (default) or `"sse"` |
| `url` | No | URL for SSE transport |

### Tool Call Flow

```
1. User sends message
2. assemble_context() includes tool definitions in ChatRequest
3. Provider streams response with ToolCallStart chunk
4. App receives tool call, routes to McpManager
5. McpClient executes tool, returns ToolCallResult
6. Result appended as a Tool message
7. Follow-up request sent with tool result in context
```

## Configuration

The full `AppConfig` structure:

| Section | Key Settings |
|---------|-------------|
| `[general]` | `default_provider`, `default_model`, `save_conversations`, `temperature`, `max_tokens`, `system_prompt`, `default_context`, `contexts_dir`, `data_dir` |
| `[ui]` | `theme`, `show_tool_panel`, `show_sidebar`, `show_timestamps`, `markdown_rendering`, `sidebar_width`, `tool_panel_width` |
| `[features]` | `latex_rendering`, `table_rendering`, `search`, `contexts`, `mcp_servers` |
| `[conversation]` | `compaction_strategy`, `compaction_threshold`, `recent_messages`, `max_checkpoints`, `budget_fraction` |
| `[usage]` | `show_token_usage`, `show_cost`, `show_context_usage`, `cost_warning_threshold`, `custom_pricing` |
| `[providers.*]` | `provider_type`, `api_key_env`, `base_url`, `models`, `default_model` |
| `[[mcp.servers]]` | `name`, `command`, `args`, `env`, `transport`, `url` |

Config validation (`AppConfig::validate()`) checks:
- Temperature range (0.0–2.0)
- Max tokens > 0
- Panel widths (1–100)
- Provider type is one of: `openai`, `anthropic`, `ollama`, `google`
- Compaction strategy is one of: `auto`, `none`, `truncation`, `summarization`
- Budget fraction and compaction threshold (0.1–1.0)

Feature toggles in `[features]` allow disabling optional functionality (LaTeX rendering, table rendering, search, contexts, MCP servers). All default to `true`.

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, terminal setup, event loop |
| `src/app.rs` | Central `App` state and action dispatch |
| `src/llm/mod.rs` | `LlmProvider` trait, `ProviderRegistry`, `build_registry()` |
| `src/llm/types.rs` | `Message`, `ChatRequest`, `StreamChunk`, `TokenUsage` |
| `src/llm/streaming.rs` | SSE stream parsing utilities |
| `src/llm/openai.rs` | OpenAI-compatible provider |
| `src/llm/anthropic.rs` | Anthropic Messages API provider |
| `src/llm/google.rs` | Google Gemini provider |
| `src/llm/ollama.rs` | Ollama local provider |
| `src/mcp/manager.rs` | `McpManager` — server lifecycle and tool routing |
| `src/mcp/client.rs` | `McpClient` — single server connection |
| `src/event/types.rs` | `Action`, `Mode`, `FocusTarget` enums |
| `src/event/keybindings.rs` | Key-to-action resolution |
| `src/config/types.rs` | `AppConfig` and all config structs |
