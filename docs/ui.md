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
| `ui.show_timestamps` | `false` | Show HH:MM:SS on messages   |

## Modes

lazyllm uses vim-style modal editing.

| Mode      | Indicator | Purpose                        |
|-----------|-----------|--------------------------------|
| **Normal**  | `NORMAL`  | Navigation, panel switching    |
| **Insert**  | `INSERT`  | Compose and send messages      |
| **Visual**  | `VISUAL`  | Scroll and copy                |
| **Command** | `COMMAND` | Execute `:` commands           |

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

## Commands

Enter command mode with `:` then type a command.

| Command            | Description          |
|--------------------|----------------------|
| `:q` / `:quit`     | Quit                 |
| `:model <id>`      | Switch model         |
| `:provider <name>` | Switch provider      |
| `:new`             | New chat             |
| `:delete` / `:del` | Delete current chat  |
| `:clear`           | Clear messages       |
| `:help`            | Toggle help overlay  |

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

| Token              | Default      | Used in                           |
|--------------------|------------- |-----------------------------------|
| `border_focused`   | `cyan`       | Active panel border               |
| `border_unfocused` | `dark_gray`  | Inactive panel border             |
| `mode_normal_bg`   | `blue`       | Status bar Normal mode background |
| `mode_normal_fg`   | `black`      | Status bar Normal mode text       |
| `mode_insert_bg`   | `green`      | Status bar Insert mode background |
| `mode_insert_fg`   | `black`      | Status bar Insert mode text       |
| `mode_visual_bg`   | `magenta`    | Status bar Visual mode background |
| `mode_visual_fg`   | `black`      | Status bar Visual mode text       |
| `mode_command_bg`  | `yellow`     | Status bar Command mode background|
| `mode_command_fg`  | `black`      | Status bar Command mode text      |
| `user_label`       | `green`      | "You:" label in chat              |
| `assistant_label`  | `blue`       | "Assistant:" label in chat        |
| `system_label`     | `yellow`     | "System:" label in chat           |
| `separator`        | `dark_gray`  | Dashed line between messages      |
| `timestamp`        | `dark_gray`  | Message timestamps                |
| `highlight`        | `cyan`       | Selected items, model name        |
| `hint_text`        | `dark_gray`  | Keybinding hints in status bar    |
| `status_message`   | `yellow`     | Status bar messages               |
| `label`            | `dark_gray`  | Labels ("Model:", "Provider:", etc)|
| `empty_state`      | `dark_gray`  | "No MCP servers" placeholder      |
| `provider_name`    | `green`      | Provider name in model bar        |
| `mcp_count`        | `yellow`     | MCP server count                  |
| `help_title`       | `cyan`       | Help overlay title                |
| `help_section`     | `yellow`     | Help overlay section headers      |
| `help_key`         | `green`      | Help overlay key column           |
| `server_name`      | `yellow`     | MCP server names in tool panel    |
| `popup_border`     | `cyan`       | Model selector & help popup border|

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
mode, input is prefixed with `:`.

### Tool Panel (right sidebar)
Displays available MCP tools grouped by server. Read-only.

### Status Bar (bottom)
Shows the current mode with a colour-coded indicator, contextual
keybinding hints, and status messages (e.g., "streaming...", "ready",
"Model updated").

### Help Overlay
Modal popup toggled with `?`. Shows all keybindings and commands.

### Model Popup
Modal popup toggled with `m`. Lists all configured provider/model
combinations for selection.
