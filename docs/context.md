# Context Management

LLMs have finite context windows. lazyllm manages what gets sent to the model using a budget-aware assembly pipeline, configurable compaction strategies, and token/cost tracking.

## Context Assembly Pipeline

The `assemble_context()` function (`src/llm/context.rs`) builds the message list for each LLM request. Messages are included in priority order:

| Priority | Source | Behavior |
|----------|--------|----------|
| 1 | System prompts | Always included |
| 2 | Context file messages | Always included |
| 3 | Tool definitions | Estimated overhead (`tokens_per_tool` per tool, default 200) |
| 4 | Compaction summary | Injected as a system message when older messages exist |
| 5 | Recent N messages | Always included (configurable, default 20) |
| 6 | Older messages | Filled newest-first until budget is exhausted |

### Budget Calculation

```
budget = context_window × budget_fraction (default 0.80)
```

Token estimation uses a `chars / 4` heuristic plus 4 tokens overhead per message. Tool call names and arguments are counted separately.

### Result

`AssembledContext` returns:

| Field | Description |
|-------|-------------|
| `messages` | Final ordered message list sent to the LLM |
| `estimated_tokens` | Estimated total token count |
| `dropped_count` | Number of older messages that didn't fit |
| `compaction_recommended` | `true` when `usage_fraction > 0.75` |
| `usage_fraction` | Fraction of the full context window used |

### Status Bar Indicator

The usage fraction drives a color-coded indicator in the status bar:

| Usage | Color |
|-------|-------|
| < 50% | Green |
| 50–75% | Yellow |
| >= 75% | Red |

## Context Files

Context files let you define reusable system prompts and reference files that are injected into every request.

### Location

```
~/.config/lazyllm/contexts/
```

Each `.toml` file in this directory is loaded at startup. The directory path is configurable via `general.contexts_dir`.

### Format

```toml
name = "rust-dev"
description = "Rust development assistant"
system_prompt = "You are an expert Rust developer. Prefer idiomatic patterns."

[[files]]
path = "~/project/README.md"
label = "Project README"

[[files]]
path = "~/project/ARCHITECTURE.md"
label = "Architecture"
```

- **`system_prompt`** — injected as a system message at the start of every request
- **`files`** — read from disk and appended to the system message. Missing files produce a `MISSING` warning line rather than failing
- **Tilde expansion** — `~` in file paths is expanded to the user's home directory

### Activating a Context

Set a default context in config:

```toml
[general]
default_context = "rust-dev"
```

Or switch at runtime with the `:context <name>` command.

## Compaction

When a conversation approaches the context window limit, compaction reduces older messages to free space. lazyllm uses a tiered, multi-strategy pipeline that adapts to the model's context window size.

See [compaction.md](compaction.md) for the full compaction architecture, pipeline details, and Pulse overlay documentation.

### Quick Reference

| Strategy | Config value | Behavior |
|----------|-------------|----------|
| None | `"none"` | No compaction; messages may be dropped silently by budget |
| Tool Clearing | `"tools"` | Replace old tool result content with placeholders (cheapest) |
| Truncation | `"truncation"` | Drop oldest messages beyond the recent window |
| Client Summarization | `"summarization"` | Summarize older messages via the LLM itself |
| Auto | `"auto"` (default) | Model-tier-aware pipeline (see compaction.md) |

### Configuration

```toml
[conversation]
compaction_strategy = "auto"       # "none", "truncation", "summarization", "auto"
compaction_threshold = 0.75        # usage fraction that triggers compaction
recent_messages = 20               # always keep this many recent messages
max_checkpoints = 5                # pre-compaction snapshots to retain
budget_fraction = 0.80             # fraction of context window to use
compaction_mode = "auto"           # "auto", "client", "server"
```

## Token Tracking

### Per-Turn Usage

Each streaming response ends with a `StreamChunk::Usage` containing:

```rust
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,       // Anthropic prompt caching
    pub cache_creation_tokens: u32,   // Anthropic cache writes
    pub cost: f64,                    // calculated USD cost
    pub duration_ms: Option<u64>,     // response latency
    pub model: Option<String>,
    pub provider: Option<String>,
}
```

Usage is tracked at three levels:
- **Per-message** — stored in the `messages` table via `MessageUsage`
- **Per-conversation** — accumulated in the `conversations` table (`total_input_tokens`, `total_cost`, etc.)
- **Per-session** — in-memory aggregate across all conversations in the current run

### Display

Configurable via the `[usage]` section:

```toml
[usage]
show_token_usage = true            # show token counts in status bar
show_cost = true                   # show cost (requires pricing data)
show_context_usage = true          # show context window usage %
cost_warning_threshold = 0.50      # per-turn cost warning (USD)
```

## Pricing

### Built-In Rates

lazyllm includes per-million-token pricing for common models:

| Provider | Models | Input | Output | Cache Read | Cache Write |
|----------|--------|-------|--------|------------|-------------|
| OpenAI | gpt-4o | $2.50 | $10.00 | — | — |
| OpenAI | gpt-4o-mini | $0.15 | $0.60 | — | — |
| Anthropic | Claude Sonnet | $3.00 | $15.00 | $0.30 | $3.75 |
| Anthropic | Claude Opus | $15.00 | $75.00 | $1.50 | $18.75 |
| Anthropic | Claude Haiku | $0.25 | $1.25 | $0.03 | $0.30 |
| Google | Gemini 2.5 Pro | $1.25 | $10.00 | — | — |
| Google | Gemini Flash | $0.075 | $0.30 | — | — |
| Ollama | (all) | — | — | — | — |

Ollama/local models return no pricing — cost tracking is disabled for them.

### Custom Pricing

Override or add pricing for any model:

```toml
[usage.custom_pricing.my-custom-model]
input_per_million = 5.0
output_per_million = 15.0
cache_read_per_million = 0.5
cache_write_per_million = 2.0
```

### Cost Formatting

- Sub-cent costs display with 4 decimal places: `$0.0035`
- Costs >= $0.01 display with 2 decimal places: `$1.23`

## Model Capabilities

lazyllm maintains a built-in registry of model capabilities used for budget calculation and feature detection:

| Model | Context Window | Max Output | Caching | Server Compaction |
|-------|---------------|------------|---------|-------------------|
| GPT-4o | 128k | 16,384 | No | No |
| GPT-4 | 8,192 | 4,096 | No | No |
| O1 | 200k | 100k | No | No |
| Claude Opus | 200k | 32,000 | Yes | Yes |
| Claude Sonnet | 200k | 64,000 | Yes | Yes |
| Claude Haiku | 200k | 8,192 | Yes | Yes |
| Gemini 2.x | 1M | 65,536 | No | No |
| Gemini 1.5 Pro | 2M | 8,192 | No | No |
| Unknown/Ollama | 128k | 4,096 | No | No |

Unknown models receive conservative defaults (128k context, 4k output).

## Key Files

| File | Purpose |
|------|---------|
| `src/llm/context.rs` | `assemble_context()` and `ContextConfig` |
| `src/llm/compaction.rs` | Compaction strategies and summarization prompt |
| `src/llm/pricing.rs` | `ModelPricing` and `get_pricing()` |
| `src/llm/capabilities.rs` | `ModelCapabilities` and `get_capabilities()` |
| `src/context/mod.rs` | `load_contexts()` from TOML files |
| `src/context/types.rs` | `Context` and `ContextFile` structs |
| `src/config/types.rs` | `ConversationConfig` and `UsageConfig` |
