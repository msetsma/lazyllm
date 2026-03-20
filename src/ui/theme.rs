use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Runtime theme with resolved ratatui Colors.
///
/// Every UI color in the application is drawn from this struct.
/// See [`ThemeConfig`] for the TOML-serialisable counterpart.
#[derive(Debug, Clone)]
pub struct Theme {
    // Borders
    pub border_focused: Color,
    pub border_unfocused: Color,

    // Mode indicators (status bar)
    pub mode_normal_bg: Color,
    pub mode_normal_fg: Color,
    pub mode_insert_bg: Color,
    pub mode_insert_fg: Color,
    pub mode_visual_bg: Color,
    pub mode_visual_fg: Color,
    pub mode_command_bg: Color,
    pub mode_command_fg: Color,

    // Chat messages
    pub user_label: Color,
    pub assistant_label: Color,
    pub system_label: Color,
    pub separator: Color,
    pub timestamp: Color,

    // Highlights & selection
    pub highlight: Color,

    // UI chrome
    pub hint_text: Color,
    pub status_message: Color,
    pub label: Color,
    pub empty_state: Color,

    // Model selector bar
    pub provider_name: Color,
    pub mcp_count: Color,

    // Help overlay
    pub help_title: Color,
    pub help_section: Color,
    pub help_key: Color,

    // Tool panel
    pub server_name: Color,

    // Popups
    pub popup_border: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border_focused: Color::Cyan,
            border_unfocused: Color::DarkGray,

            mode_normal_bg: Color::Blue,
            mode_normal_fg: Color::Black,
            mode_insert_bg: Color::Green,
            mode_insert_fg: Color::Black,
            mode_visual_bg: Color::Magenta,
            mode_visual_fg: Color::Black,
            mode_command_bg: Color::Yellow,
            mode_command_fg: Color::Black,

            user_label: Color::Green,
            assistant_label: Color::Blue,
            system_label: Color::Yellow,
            separator: Color::DarkGray,
            timestamp: Color::DarkGray,

            highlight: Color::Cyan,

            hint_text: Color::DarkGray,
            status_message: Color::Yellow,
            label: Color::DarkGray,
            empty_state: Color::DarkGray,

            provider_name: Color::Green,
            mcp_count: Color::Yellow,

            help_title: Color::Cyan,
            help_section: Color::Yellow,
            help_key: Color::Green,

            server_name: Color::Yellow,

            popup_border: Color::Cyan,
        }
    }
}

/// TOML-serialisable theme configuration.
///
/// Every field is optional; omitted values fall back to the built-in default
/// theme. Colour values accept:
///
/// - Named colours: `"red"`, `"cyan"`, `"dark_gray"`, `"light_blue"`, ...
/// - Hex RGB:       `"#ff0000"`, `"#f00"`
/// - 256-colour:    `"0"`..`"255"`
/// - Reset:         `"reset"` / `"default"`
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ThemeConfig {
    pub border_focused: Option<String>,
    pub border_unfocused: Option<String>,

    pub mode_normal_bg: Option<String>,
    pub mode_normal_fg: Option<String>,
    pub mode_insert_bg: Option<String>,
    pub mode_insert_fg: Option<String>,
    pub mode_visual_bg: Option<String>,
    pub mode_visual_fg: Option<String>,
    pub mode_command_bg: Option<String>,
    pub mode_command_fg: Option<String>,

    pub user_label: Option<String>,
    pub assistant_label: Option<String>,
    pub system_label: Option<String>,
    pub separator: Option<String>,
    pub timestamp: Option<String>,

    pub highlight: Option<String>,

    pub hint_text: Option<String>,
    pub status_message: Option<String>,
    pub label: Option<String>,
    pub empty_state: Option<String>,

    pub provider_name: Option<String>,
    pub mcp_count: Option<String>,

    pub help_title: Option<String>,
    pub help_section: Option<String>,
    pub help_key: Option<String>,

    pub server_name: Option<String>,

    pub popup_border: Option<String>,
}

impl ThemeConfig {
    /// Resolve into a [`Theme`], falling back to defaults for unset values.
    pub fn resolve(&self) -> Theme {
        let d = Theme::default();
        Theme {
            border_focused: resolve_field(&self.border_focused, d.border_focused),
            border_unfocused: resolve_field(&self.border_unfocused, d.border_unfocused),
            mode_normal_bg: resolve_field(&self.mode_normal_bg, d.mode_normal_bg),
            mode_normal_fg: resolve_field(&self.mode_normal_fg, d.mode_normal_fg),
            mode_insert_bg: resolve_field(&self.mode_insert_bg, d.mode_insert_bg),
            mode_insert_fg: resolve_field(&self.mode_insert_fg, d.mode_insert_fg),
            mode_visual_bg: resolve_field(&self.mode_visual_bg, d.mode_visual_bg),
            mode_visual_fg: resolve_field(&self.mode_visual_fg, d.mode_visual_fg),
            mode_command_bg: resolve_field(&self.mode_command_bg, d.mode_command_bg),
            mode_command_fg: resolve_field(&self.mode_command_fg, d.mode_command_fg),
            user_label: resolve_field(&self.user_label, d.user_label),
            assistant_label: resolve_field(&self.assistant_label, d.assistant_label),
            system_label: resolve_field(&self.system_label, d.system_label),
            separator: resolve_field(&self.separator, d.separator),
            timestamp: resolve_field(&self.timestamp, d.timestamp),
            highlight: resolve_field(&self.highlight, d.highlight),
            hint_text: resolve_field(&self.hint_text, d.hint_text),
            status_message: resolve_field(&self.status_message, d.status_message),
            label: resolve_field(&self.label, d.label),
            empty_state: resolve_field(&self.empty_state, d.empty_state),
            provider_name: resolve_field(&self.provider_name, d.provider_name),
            mcp_count: resolve_field(&self.mcp_count, d.mcp_count),
            help_title: resolve_field(&self.help_title, d.help_title),
            help_section: resolve_field(&self.help_section, d.help_section),
            help_key: resolve_field(&self.help_key, d.help_key),
            server_name: resolve_field(&self.server_name, d.server_name),
            popup_border: resolve_field(&self.popup_border, d.popup_border),
        }
    }
}

fn resolve_field(value: &Option<String>, fallback: Color) -> Color {
    match value {
        Some(s) => parse_color(s).unwrap_or(fallback),
        None => fallback,
    }
}

/// Parse a colour string into a ratatui [`Color`].
///
/// Supported formats:
///
/// | Format | Example |
/// |--------|---------|
/// | Named  | `"red"`, `"dark_gray"`, `"light_blue"` |
/// | Hex    | `"#ff0000"`, `"#f00"` |
/// | 256    | `"0"` .. `"255"` |
/// | Reset  | `"reset"`, `"default"` |
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim().to_lowercase();

    // Hex colours
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    // 256-colour index
    if let Ok(n) = s.parse::<u8>() {
        return Some(Color::Indexed(n));
    }

    // Named colours
    match s.as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "dark_gray" | "dark_grey" | "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "light_red" | "lightred" => Some(Color::LightRed),
        "light_green" | "lightgreen" => Some(Color::LightGreen),
        "light_yellow" | "lightyellow" => Some(Color::LightYellow),
        "light_blue" | "lightblue" => Some(Color::LightBlue),
        "light_magenta" | "lightmagenta" => Some(Color::LightMagenta),
        "light_cyan" | "lightcyan" => Some(Color::LightCyan),
        "reset" | "default" => Some(Color::Reset),
        _ => None,
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    match hex.len() {
        // #RGB shorthand → expand each nibble
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        // #RRGGBB
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Load a theme from a TOML file, merging with defaults for missing fields.
pub fn load_theme_file(path: &Path) -> Theme {
    match std::fs::read_to_string(path) {
        Ok(contents) => match toml::from_str::<ThemeConfig>(&contents) {
            Ok(config) => config.resolve(),
            Err(e) => {
                tracing::warn!("Failed to parse theme file {}: {e}", path.display());
                Theme::default()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read theme file {}: {e}", path.display());
            Theme::default()
        }
    }
}

/// Load a theme by name.
///
/// Looks for `~/.config/lazyllm/themes/<name>.toml`. Falls back to the
/// built-in default when the name is `"default"`, empty, or the file is
/// missing.
pub fn load_theme(name: &str) -> Theme {
    if name == "default" || name.is_empty() {
        return Theme::default();
    }

    let theme_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".config"))
        .join("lazyllm")
        .join("themes")
        .join(format!("{name}.toml"));

    if theme_path.exists() {
        load_theme_file(&theme_path)
    } else {
        tracing::warn!(
            "Theme '{name}' not found at {}, using default",
            theme_path.display()
        );
        Theme::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Ensures named colors like "red", "cyan", and "dark_gray" parse to correct Color values.
    #[test]
    fn parse_named_colors() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("light_red"), Some(Color::LightRed));
        assert_eq!(parse_color("reset"), Some(Color::Reset));
    }

    /// Verifies 3-digit and 6-digit hex color strings parse to correct RGB values.
    #[test]
    fn parse_hex_colors() {
        assert_eq!(parse_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_color("#0000ff"), Some(Color::Rgb(0, 0, 255)));
        assert_eq!(parse_color("#f00"), Some(Color::Rgb(255, 0, 0)));
    }

    /// Verifies numeric strings "0" and "255" parse to 256-color indexed values.
    #[test]
    fn parse_indexed_colors() {
        assert_eq!(parse_color("0"), Some(Color::Indexed(0)));
        assert_eq!(parse_color("255"), Some(Color::Indexed(255)));
    }

    /// Ensures invalid color strings like unknown names and malformed hex return None.
    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_color("not_a_color"), None);
        assert_eq!(parse_color("#gggggg"), None);
        assert_eq!(parse_color("#1234"), None);
    }

    /// Verifies color parsing is case-insensitive for both named and hex colors.
    #[test]
    fn parse_case_insensitive() {
        assert_eq!(parse_color("RED"), Some(Color::Red));
        assert_eq!(parse_color("DarkGray"), Some(Color::DarkGray));
        assert_eq!(parse_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
    }

    /// Verifies the default theme has cyan focused borders, dark gray unfocused, and correct role colors.
    #[test]
    fn default_theme_values() {
        let theme = Theme::default();
        assert_eq!(theme.border_focused, Color::Cyan);
        assert_eq!(theme.border_unfocused, Color::DarkGray);
        assert_eq!(theme.user_label, Color::Green);
        assert_eq!(theme.assistant_label, Color::Blue);
    }

    /// Ensures an empty ThemeConfig resolves to default theme values.
    #[test]
    fn theme_config_resolve_uses_defaults() {
        let config = ThemeConfig::default();
        let theme = config.resolve();
        assert_eq!(theme.border_focused, Color::Cyan);
    }

    /// Verifies ThemeConfig overrides only the specified color while keeping other defaults.
    #[test]
    fn theme_config_resolve_overrides() {
        let config = ThemeConfig {
            border_focused: Some("red".to_string()),
            ..Default::default()
        };
        let theme = config.resolve();
        assert_eq!(theme.border_focused, Color::Red);
        assert_eq!(theme.border_unfocused, Color::DarkGray); // untouched
    }

    /// Ensures an invalid color string in ThemeConfig falls back to the default color.
    #[test]
    fn theme_config_invalid_color_falls_back() {
        let config = ThemeConfig {
            border_focused: Some("not_a_color".to_string()),
            ..Default::default()
        };
        let theme = config.resolve();
        assert_eq!(theme.border_focused, Color::Cyan); // default
    }

    /// Ensures load_theme("default") returns the default theme.
    #[test]
    fn load_theme_default_returns_default() {
        let theme = load_theme("default");
        assert_eq!(theme.border_focused, Color::Cyan);
    }

    /// Ensures load_theme("") returns the default theme.
    #[test]
    fn load_theme_empty_returns_default() {
        let theme = load_theme("");
        assert_eq!(theme.border_focused, Color::Cyan);
    }

    /// Ensures a non-existent theme name gracefully falls back to the default theme.
    #[test]
    fn load_theme_missing_returns_default() {
        let theme = load_theme("nonexistent_theme_xyz");
        assert_eq!(theme.border_focused, Color::Cyan);
    }

    /// Verifies a theme file with partial overrides applies them while keeping other defaults.
    #[test]
    fn load_theme_file_partial() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("partial.toml");
        std::fs::write(
            &path,
            r##"
border_focused = "#ff0000"
user_label = "magenta"
"##,
        )
        .unwrap();

        let theme = load_theme_file(&path);
        assert_eq!(theme.border_focused, Color::Rgb(255, 0, 0));
        assert_eq!(theme.user_label, Color::Magenta);
        assert_eq!(theme.border_unfocused, Color::DarkGray); // default
    }

    /// Ensures an invalid TOML theme file falls back to the default theme without panicking.
    #[test]
    fn load_theme_file_invalid_toml_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.toml");
        std::fs::write(&path, "{{{{invalid").unwrap();

        let theme = load_theme_file(&path);
        assert_eq!(theme.border_focused, Color::Cyan);
    }

    /// Ensures ThemeConfig survives a TOML serialize/deserialize roundtrip.
    #[test]
    fn theme_config_roundtrip_toml() {
        let config = ThemeConfig {
            border_focused: Some("red".to_string()),
            highlight: Some("#00ff00".to_string()),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: ThemeConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }
}
