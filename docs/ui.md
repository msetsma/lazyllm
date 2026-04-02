# lazyllm UI Reference

## Layout

```
+---------------------------------------------+
| Model Selector (2 lines)                    |
+--------+------------------------+-----------+
| Chats  | Chat View              | Tools     |
| (25w)  | (fill, min 20)         | (20w)     |
+--------+------------------------+-----------+
| Input Box (3 lines)                         |
+---------------------------------------------+
| Status Bar (1 line)                         |
+---------------------------------------------+
```

### Configurable dimensions

| Setting              | Default | Description                  |
|----------------------|---------|------------------------------|
| `ui.sidebar_width`   | `25`    | Chat list panel width        |
| `ui.tool_panel_width`| `20`    | Tool panel width             |
| `ui.show_tool_panel` | `true`  | Toggle tool panel visibility |
| `ui.show_timestamps` | `false` | Show HH:MM:SS on messages    |

## Modes

lazyllm uses vim-style modal editing.

| Mode      | Indicator | Purpose                        |
|-----------|-----------|--------------------------------|
| **Normal**  | `NORMAL`  | Navigation, panel switching    |
| **Insert**  | `INSERT`  | Compose and send messages      |
| **Visual**  | `VISUAL`  | Scroll and copy                |
| **Command** | `COMMAND` | Execute `:` commands           |
| **Search**  | `SEARCH`  | Search within conversation     |

## Keybindings

### Global

| Key      | Action |
|----------|--------|
| `Ctrl+C` | Quit   |

### Normal mode

| Key              | Action                  |
|------------------|-------------------------|
| `q`              | Quit                    |
| `i`              | Enter Insert mode       |
| `v`              | Enter Visual mode       |
| `:`              | Enter Command mode      |
| `/`              | Enter Search mode       |
| `Tab`            | Focus next panel        |
| `Shift+Tab`      | Focus previous panel    |
| `h` / `Left`     | Focus previous panel    |
| `l` / `Right`    | Focus next panel        |
| `j` / `Down`     | Scroll down             |
| `k` / `Up`       | Scroll up               |
| `Enter`          | Select item             |
| `n`              | New chat                |
| `d`              | Delete chat             |
| `m`              | Toggle model selector   |
| `t`              | Toggle tool panel       |
| `P`              | Toggle Pulse overlay    |
| `?`              | Toggle help overlay     |

### Insert mode

| Key         | Action              |
|-------------|---------------------|
| `Esc`       | Return to Normal    |
| `Enter`     | Send message        |
| `Backspace` | Delete character    |
| *(typing)*  | Insert character    |

### Visual mode

| Key            | Action              |
|----------------|---------------------|
| `Esc`          | Return to Normal    |
| `j` / `Down`   | Scroll down         |
| `k` / `Up`     | Scroll up           |
| `y`            | Copy last response  |

### Command mode

| Key         | Action              |
|-------------|---------------------|
| `Esc`       | Cancel              |
| `Enter`     | Execute command     |
| `Backspace` | Delete character    |
| *(typing)*  | Insert character    |

### Search mode

| Key            | Action              |
|----------------|---------------------|
| `Esc`          | Cancel search       |
| `Enter`        | Next match          |
| `Down`         | Next match          |
| `Up`           | Previous match      |
| `Backspace`    | Delete character    |
| *(typing)*     | Insert character    |

### Pulse overlay

The Pulse overlay is opened with `P` in Normal mode (or `:pulse`). All keys
are consumed by the overlay while it is open.

| Key       | Action                              |
|-----------|-------------------------------------|
| `P` / `Esc` / `q` | Close overlay              |
| `j` / `Down` | Scroll down                    |
| `k` / `Up`   | Scroll up                      |
| `c`       | Trigger compaction                  |
| `C`       | Trigger compaction with custom prompt |
| `t`       | Clear tool results (cheapest compaction) |
| `u`       | Undo last compaction                |
| `p`       | Pin / unpin selected message        |
| `n`       | Open session notes editor           |
| `s`       | Cycle compaction mode               |

## Commands

Enter command mode with `:` then type a command.

### Navigation & chat

| Command                | Aliases       | Description                        |
|------------------------|---------------|------------------------------------|
| `:q` / `:quit`         |               | Quit                               |
| `:new`                 |               | New chat                           |
| `:delete`              | `:del`        | Delete current chat                |
| `:clear`               |               | Clear messages in current chat     |
| `:help`                |               | Toggle help overlay                |
| `:pulse`               |               | Toggle Pulse overlay               |

### Model & provider

| Command                | Description                                    |
|------------------------|------------------------------------------------|
| `:model <id>`          | Switch to a specific model                     |
| `:provider <name>`     | Switch to a different provider                 |

### Context

| Command                | Aliases       | Description                        |
|------------------------|---------------|------------------------------------|
| `:context <name>`      | `:ctx <name>` | Load a context file                |
| `:context none`        | `:ctx`        | Clear the active context           |

### Compaction & checkpoints

| Command                | Description                                    |
|------------------------|------------------------------------------------|
| `:compact`             | Trigger compaction immediately                 |
| `:compact <prompt>`    | Compact with a custom summarization prompt     |
| `:checkpoints`         | List available checkpoints for this chat       |
| `:restore`             | Restore the most recent checkpoint             |
| `:restore <id>`        | Restore a specific checkpoint by ID           |

### Usage & export

| Command                | Aliases       | Description                        |
|------------------------|---------------|------------------------------------|
| `:usage`               | `:tokens`     | Show token usage summary           |
| `:spend`               | `:cost`       | Show cost breakdown                |
| `:export`              |               | Export conversation to JSON        |
| `:import <path>`       |               | Import conversation from JSON      |

### Session

| Command                | Description                                    |
|------------------------|------------------------------------------------|
| `:notes`               | Edit session notes                             |
| `:pin`                 | Pin / unpin the selected message               |

### Runtime config

`:set <key> <value>` — change a setting for the current session.

| Key                  | Values            | Description                       |
|----------------------|-------------------|-----------------------------------|
| `temperature`        | `0.0` – `2.0`     | Model sampling temperature        |
| `compaction`         | `auto` / `truncation` / `summarization` / `none` | Compaction strategy |
| `recent_messages`    | integer           | Minimum messages kept after compaction |
| `show_cost`          | `true` / `false`  | Show per-turn cost                |
| `show_tokens`        | `true` / `false`  | Show token counts                 |
| `timestamps`         | `true` / `false`  | Show message timestamps           |
| `system_prompt`      | text / `none`     | Set or clear the system prompt    |

## Focus Management

Panels cycle in this order:

```
ChatList -> ChatView -> ToolPanel -> Input -> ChatList
```

- `Tab` / `Shift+Tab` cycle forward / backward
- `h` / `l` (or arrow keys) move left / right
- Entering Insert or Command mode auto-focuses the Input panel

## Theme Customization

lazyllm supports full colour customisation via theme files, following the
same pattern as tools like helix and lazygit.

### How it works

1. Set `ui.theme` in your config to the name of a theme
2. Theme files live at `~/.config/lazyllm/themes/<name>.toml`
3. The built-in `"default"` theme is used when no file is found
4. Every field in a theme file is optional — omitted values use the default

### Quick start

```toml
# ~/.config/lazyllm/config.toml
[ui]
theme = "dracula"
```

```toml
# ~/.config/lazyllm/themes/dracula.toml
border_focused = "#bd93f9"
border_unfocused = "#44475a"
mode_normal_bg = "#6272a4"
mode_normal_fg = "#f8f8f2"
mode_insert_bg = "#50fa7b"
mode_insert_fg = "#282a36"
mode_visual_bg = "#ff79c6"
mode_visual_fg = "#282a36"
mode_command_bg = "#f1fa8c"
mode_command_fg = "#282a36"
user_label = "#50fa7b"
assistant_label = "#8be9fd"
system_label = "#f1fa8c"
separator = "#44475a"
timestamp = "#6272a4"
highlight = "#bd93f9"
hint_text = "#6272a4"
status_message = "#f1fa8c"
label = "#6272a4"
empty_state = "#44475a"
provider_name = "#50fa7b"
mcp_count = "#f1fa8c"
help_title = "#8be9fd"
help_section = "#f1fa8c"
help_key = "#50fa7b"
server_name = "#f1fa8c"
popup_border = "#bd93f9"
```

### Colour formats

| Format      | Example             | Notes                        |
|-------------|---------------------|------------------------------|
| Named       | `"cyan"`            | Standard terminal colours    |
| Hex RGB     | `"#ff0000"`         | True colour                  |
| Hex short   | `"#f00"`            | Expands to `#ff0000`         |
| 256-colour  | `"42"`              | xterm-256 palette index      |
| Reset       | `"reset"`           | Terminal default              |

### Named colours

`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`,
`gray` / `grey`, `dark_gray` / `dark_grey`,
`light_red`, `light_green`, `light_yellow`, `light_blue`,
`light_magenta`, `light_cyan`

### All theme tokens

#### Panels & borders

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `border_focused`   | `cyan`       | Active panel border               |
| `border_unfocused` | `dark_gray`  | Inactive panel border             |

#### Status bar modes

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `mode_normal_bg`   | `blue`       | Normal mode background            |
| `mode_normal_fg`   | `black`      | Normal mode text                  |
| `mode_insert_bg`   | `green`      | Insert mode background            |
| `mode_insert_fg`   | `black`      | Insert mode text                  |
| `mode_visual_bg`   | `magenta`    | Visual mode background            |
| `mode_visual_fg`   | `black`      | Visual mode text                  |
| `mode_command_bg`  | `yellow`     | Command mode background           |
| `mode_command_fg`  | `black`      | Command mode text                 |

#### Chat messages

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `user_label`       | `green`      | "You:" label                      |
| `user_msg_bg`      | *(none)*     | User message bubble background    |
| `assistant_label`  | `blue`       | "Assistant:" label                |
| `assistant_msg_bg` | *(none)*     | Assistant message bubble background |
| `system_label`     | `yellow`     | "System:" label                   |
| `separator`        | `dark_gray`  | Dashed line between messages      |
| `timestamp`        | `dark_gray`  | Message timestamps                |
| `visual_select`    | `cyan`       | Visual mode selection highlight   |

#### General UI

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `highlight`        | `cyan`       | Selected items, model name        |
| `hint_text`        | `dark_gray`  | Keybinding hints in status bar    |
| `status_message`   | `yellow`     | Status bar messages               |
| `label`            | `dark_gray`  | Labels ("Model:", "Provider:", …) |
| `empty_state`      | `dark_gray`  | "No MCP servers" placeholder      |
| `popup_border`     | `cyan`       | Model selector & help popup border|

#### Model selector bar

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `provider_name`    | `green`      | Provider name                     |
| `mcp_count`        | `yellow`     | MCP server count                  |

#### Help overlay

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `help_title`       | `cyan`       | Help overlay title                |
| `help_section`     | `yellow`     | Section headers                   |
| `help_key`         | `green`      | Key column                        |

#### Tool panel

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `server_name`      | `yellow`     | MCP server names                  |

#### Context health (Pulse overlay & status bar)

Health colours reflect context window usage: fresh < 30%, working 30–60%,
warm 60–75%, hot ≥ 75%.

| Token              | Default      | Used in                           |
|--------------------|--------------|-----------------------------------|
| `health_fresh`     | `green`      | Usage < 30%                       |
| `health_working`   | `cyan`       | Usage 30–60%                      |
| `health_warm`      | `yellow`     | Usage 60–75%                      |
| `health_hot`       | `red`        | Usage ≥ 75%                       |

#### Pulse overlay — budget breakdown

| Token                | Default      | Used in                           |
|----------------------|--------------|-----------------------------------|
| `budget_system`      | `blue`       | System prompt token bar           |
| `budget_context`     | `cyan`       | Context files token bar           |
| `budget_tools`       | `magenta`    | Tool definitions token bar        |
| `budget_compaction`  | `yellow`     | Compaction summary token bar      |
| `budget_messages`    | `green`      | Message history token bar         |
| `budget_free`        | `dark_gray`  | Remaining free tokens bar         |

#### Pulse overlay — chrome

| Token                | Default      | Used in                           |
|----------------------|--------------|-----------------------------------|
| `pulse_border`       | `cyan`       | Pulse overlay border              |
| `pulse_section_title`| `yellow`     | Section header text               |
| `pulse_pin_icon`     | `magenta`    | Pinned message icon               |
| `pulse_note_border`  | `blue`       | Notes editor border               |

### Minimal override example

You only need to set the values you want to change:

```toml
# ~/.config/lazyllm/themes/solarized-light.toml
border_focused = "#268bd2"
highlight = "#268bd2"
user_label = "#859900"
assistant_label = "#268bd2"
separator = "#93a1a1"
```

## Configuration Reference

Full `~/.config/lazyllm/config.toml` example:

```toml
[general]
default_provider = "openai"
default_model = "gpt-4o"
save_conversations = true
# data_dir = "/custom/path"  # defaults to ~/.local/share/lazyllm

[ui]
theme = "default"          # name of theme file, or "default" for built-in
show_tool_panel = true
show_timestamps = false
sidebar_width = 25
tool_panel_width = 20

[providers.openai]
provider_type = "openai"
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
models = ["gpt-4o", "gpt-4o-mini"]

[providers.anthropic]
provider_type = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
models = ["claude-sonnet-4-20250514"]

[providers.ollama]
provider_type = "ollama"
base_url = "http://localhost:11434"
models = ["llama3", "codellama"]

[providers.google]
provider_type = "google"
api_key_env = "GOOGLE_API_KEY"
models = ["gemini-2.5-pro"]

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

## Components

### Model Selector (top bar)
Displays the active model, provider, and MCP server count. Read-only.

### Chat List (left sidebar)
Lists all conversations. Navigate with `j`/`k`, select with `Enter`,
create with `n`, delete with `d`.

### Chat View (center)
Renders conversation messages. Assistant messages use full markdown
rendering (bold, italic, code blocks with syntax highlighting, lists,
links). User and system messages render as plain text.

Auto-scrolls to the bottom. Manual scroll with `j`/`k` in Normal or
Visual mode.

### Input Box (bottom)
Text composition area. Shows the current mode in the title. In Command
mode, input is prefixed with `:`. In Search mode, input is prefixed with `/`.

### Tool Panel (right sidebar)
Displays available MCP tools grouped by server. Toggled with `t`. Read-only.

### Status Bar (bottom)
Shows the current mode with a colour-coded indicator, contextual
keybinding hints, and status messages (e.g., "streaming...", "ready",
"Model updated").

### Help Overlay
Modal popup toggled with `?`. Shows all keybindings and commands.

### Model Popup
Modal popup toggled with `m`. Lists all configured provider/model
combinations for selection.

### Pulse Overlay
Modal popup toggled with `P` or `:pulse`. Displays a real-time dashboard
of context window health, broken into sections:

- **Context budget** — token allocation bar showing system prompt, context
  files, tools, compaction summary, pinned messages, message history, and
  free space. Coloured by health state (fresh → working → warm → hot).
- **Session stats** — turn count, total input/output tokens, cumulative cost,
  cache hit rate, estimated cost per message, compaction count.
- **Pinned messages** — list of messages protected from compaction.
- **Session notes** — freeform notes attached to this conversation.
- **Compaction history** — timestamped log of previous compactions.

Actions available from within the overlay: compact (`c`/`C`), clear tool
results (`t`), undo (`u`), pin/unpin message (`p`), edit notes (`n`), cycle
compaction mode (`s`). See [Pulse overlay keybindings](#pulse-overlay) above.

### Notes Editor
Inline text editor opened via `:notes` or `n` inside the Pulse overlay.
Edits are saved to the active conversation on close.
