# lazyllm — Implementation Plan

## Overview

Three interconnected systems to build, in dependency order:

1. **SQLite Storage Layer** — Replace JSON file persistence with SQLite
2. **Token Usage Tracking** — Capture, accumulate, and display token/cost data across all providers
3. **Context Window Management** — Provider-aware hybrid strategy for keeping conversations within context limits

---

## Phase 1: SQLite Storage Layer

Replace `JsonStore` with a `SqliteStore` backed by a single `lazyllm.db` file using `rusqlite` with the `bundled` feature (compiles SQLite into the binary — zero user dependencies).

**Three tables:**

- `conversations` — One row per conversation. Stores metadata (title, model, provider, context name, timestamps) and aggregate usage totals (total input/output/cache tokens, total cost, turn count, context estimate). This is what the sidebar queries — must be fast to list and sort.
- `messages` — One row per message. Stores role, content, message index, per-turn usage (input/output/cache tokens, cost, duration), model used, and a `metadata_json` column for provider-specific data (tool call IDs, compaction blocks, thinking signatures, etc.).
- `checkpoints` — Pre-compaction snapshots. Stores a JSON snapshot of the message history at the time of compaction, with a reason and timestamp. Capped per conversation.

**Key behaviors:**
- Appending a message and updating conversation totals should be a single atomic transaction
- Use WAL journal mode for concurrent reads/writes and crash safety
- The in-memory `Conversation` and `Vec<Message>` remain as the working runtime state; the DB is the persistence layer
- Add `export` and `import` CLI commands for JSON interop (debugging, backup)

---

## Phase 2: Token Usage Tracking

Capture token usage that every LLM API already returns in its responses, calculate costs, and surface it to the user.

### Data Capture

- Add a `Usage` variant to `StreamChunk` carrying input tokens, output tokens, cache creation tokens, cache read tokens, duration, model, and provider
- Each provider adapter extracts usage from its API's response format and sends it as this chunk. All token fields are optional — local providers may not report all of them, and the system must degrade gracefully (display "—", skip cost calc)
- Anthropic: usage comes in `message_start` and final `message_delta` stream events
- OpenAI-compatible: usage comes in the final stream chunk or non-streamed response
- Ollama/local: extract whatever is available (`prompt_eval_count`, `eval_count`), leave the rest as None

### Cost Calculation

- Maintain a model pricing registry mapping model names to per-million-token prices (input, output, cache write multiplier, cache read multiplier)
- Ship built-in defaults for major cloud models. Local/unknown models default to zero cost
- Allow user overrides via a custom pricing file in config
- Compute cost per turn when usage data arrives, store it alongside the message

### Context Size Estimation

- Estimate the token count of the full message history that would be sent on the next request
- Default: character count / 4 heuristic (~85% accurate for English BPE tokenizers)
- Optional precision mode for Anthropic: call their free `/v1/messages/count_tokens` endpoint when estimate is above 60% of context window
- Store the estimate on the conversation and use it to drive compaction thresholds and UI warnings

### Display

- Persistent TUI status bar showing: model, context usage (tokens and percentage), last turn cost, session total cost
- Color-code context percentage (green/yellow/red at configurable thresholds)
- `/usage` command: detailed per-conversation breakdown
- `/spend` command: cross-conversation cost report grouped by model, queryable by time range (pulls from the messages table)

---

## Phase 3: Context Window Management

Stop sending the entire conversation history on every request. Build a provider-aware pipeline that assembles context within budget.

### Model Capabilities Registry

Each model needs known capabilities: context window size, max output tokens, and feature flags (supports server compaction, supports cache control, supports system messages). Ship defaults for known models, allow user overrides in config. Unknown models get conservative defaults (8k context, no special features).

### Context Assembly Pipeline

Replace the current flat message assembly with a budget-aware pipeline. Compute the available input budget (context window minus max output tokens minus a safety margin), then fill it in priority order:

1. System prompts (global + context) — always included, warn if they consume too much of the budget
2. Tool definitions — always included if active
3. Recent messages — last N messages always kept verbatim (N is configurable)
4. Compaction summary — if one exists from a prior compaction, include it
5. Older messages — fill remaining budget newest-first until budget is exhausted

### Compaction Strategies

Define a strategy trait with two methods: whether to compact (given current context estimate and threshold), and how to compact (returning a new message list and optional summary).

**Four strategies:**

- **Server Compaction** — For Anthropic only. Injects `context_management` parameters into the outgoing API request. The server handles summarization and returns compaction blocks in the response. The app must preserve full response content (not just text) to maintain compaction state. Minimal code, highest quality summaries.
- **Client Summarization** — Works with any provider. When threshold is hit, sends older messages to a summarization model (configurable — can be a cheaper/faster model) in a background task. Replaces older messages with a summary. Saves a checkpoint before summarizing.
- **Truncation** — Simplest fallback. Drops older messages beyond the recent window. No extra inference cost. Good default for local models with small context windows.
- **None** — Current behavior. Send everything, let the API error on overflow. The app should catch overflow errors and suggest enabling a strategy.

**Auto-selection when strategy is set to "auto":**
- Anthropic → server compaction
- OpenAI → client summarization
- Local providers → truncation

### Checkpoint Management

Before any compaction (auto or manual), save a snapshot of the current messages to the checkpoints table. Cap at a configurable max per conversation. Add `/checkpoints` and `/restore` commands so users can rewind past a compaction.

### Manual Compaction

Add a `/compact` command that triggers the active strategy immediately, with an optional instruction parameter to guide what the summary should focus on.

---

## Phase 4: User-Configurable Settings

All tuneable parameters exposed through config with sensible defaults so zero configuration works out of the box.

**Conversation settings:**
- Compaction strategy (auto / server_compaction / client_summarization / truncation / none)
- Compaction threshold percentage (when to trigger)
- Number of recent messages to always preserve
- Summarization model (for client summarization; empty = use conversation model)
- Max checkpoints per conversation
- System prompt budget percentage (warn if system prompts exceed this share of context)

**Usage settings:**
- Enable/disable token tracking
- Show/hide status bar
- Show/hide per-turn cost
- Custom pricing file path
- Context warning threshold percentage (separate from compaction threshold — lets users get a heads-up before compaction kicks in)

**Model settings:**
- Custom capabilities file path
- Inline per-model overrides for context window, max output tokens, and feature flags

**Per-conversation overrides:**
- Allow setting overrides on individual conversations (stored in the conversations table) that merge on top of global config. Useful when switching between models with different context windows.

---

## Phase 5: Integration & Polish

- Wire the full pipeline into `start_streaming()`: load config → check compaction → assemble context → build request → send → process usage → persist
- Error handling: catch context overflow errors gracefully, fall back from summarization to truncation on failure, handle missing usage data without panics, handle DB write failures without losing in-memory state
- Add all new TUI commands: `/usage`, `/spend`, `/compact`, `/checkpoints`, `/restore`, `/tokens`, `/export`, `/set`
- Tests: unit tests for context assembly budgeting, cost calculation, each compaction strategy; integration tests for the storage layer and the full pipeline

---

## Implementation Order

```
Phase 1 (SQLite)           ← Foundation, everything depends on this
Phase 2 (Token Tracking)   ← Immediate user-visible value
Phase 3 (Context Mgmt)     ← Most complex, builds on Phase 1 & 2
Phase 4 (Settings)         ← Can be done incrementally alongside Phase 2 & 3
Phase 5 (Integration)      ← Final wiring, error handling, tests
```

Within Phase 3, implement strategies in this order: truncation (simplest, good fallback), server compaction (low code, high value for Anthropic users), client summarization (most complex, needs background task and model call).