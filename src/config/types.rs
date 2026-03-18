use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralConfig {
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default = "default_true")]
    pub save_conversations: bool,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub default_context: Option<String>,
    #[serde(default = "default_contexts_dir")]
    pub contexts_dir: PathBuf,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_provider: default_provider(),
            default_model: default_model(),
            save_conversations: true,
            data_dir: default_data_dir(),
            default_context: None,
            contexts_dir: default_contexts_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub show_tool_panel: bool,
    #[serde(default)]
    pub show_timestamps: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    #[serde(default = "default_tool_panel_width")]
    pub tool_panel_width: u16,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            show_tool_panel: true,
            show_timestamps: false,
            sidebar_width: default_sidebar_width(),
            tool_panel_width: default_tool_panel_width(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    /// Provider type: "openai", "anthropic", "ollama", "google".
    /// Defaults to "openai" for backward compatibility.
    #[serde(default = "default_provider_type")]
    pub provider_type: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

fn default_provider_type() -> String {
    "openai".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

fn default_provider() -> String {
    "openai".to_string()
}

fn default_model() -> String {
    "gpt-4o".to_string()
}

fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazyllm")
}

fn default_contexts_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("lazyllm")
        .join("contexts")
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "default".to_string()
}

fn default_sidebar_width() -> u16 {
    25
}

fn default_tool_panel_width() -> u16 {
    20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_values() {
        let config = AppConfig::default();
        assert_eq!(config.general.default_provider, "openai");
        assert_eq!(config.general.default_model, "gpt-4o");
        assert!(config.general.save_conversations);
        assert_eq!(config.ui.sidebar_width, 25);
        assert_eq!(config.ui.tool_panel_width, 20);
        assert!(config.ui.show_tool_panel);
        assert!(!config.ui.show_timestamps);
        assert!(config.providers.is_empty());
        assert!(config.mcp.servers.is_empty());
    }

    #[test]
    fn config_deserializes_from_minimal_toml() {
        let toml_str = r#"
[general]
default_provider = "anthropic"
default_model = "claude-sonnet-4-20250514"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.default_provider, "anthropic");
        assert_eq!(config.general.default_model, "claude-sonnet-4-20250514");
        // Defaults should fill in the rest
        assert!(config.general.save_conversations);
        assert_eq!(config.ui.sidebar_width, 25);
    }

    #[test]
    fn config_deserializes_with_providers() {
        let toml_str = r#"
[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
models = ["gpt-4o", "gpt-4o-mini"]

[providers.ollama]
base_url = "http://localhost:11434"
models = []
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.providers.len(), 2);
        let openai = &config.providers["openai"];
        assert_eq!(openai.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(openai.models.len(), 2);
        let ollama = &config.providers["ollama"];
        assert_eq!(
            ollama.base_url.as_deref(),
            Some("http://localhost:11434")
        );
    }

    #[test]
    fn config_deserializes_with_mcp_servers() {
        let toml_str = r#"
[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[[mcp.servers]]
name = "custom"
command = "/usr/bin/my-server"
transport = "sse"
url = "http://localhost:8080/sse"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.servers.len(), 2);
        assert_eq!(config.mcp.servers[0].name, "filesystem");
        assert_eq!(config.mcp.servers[0].args.len(), 3);
        assert_eq!(config.mcp.servers[1].transport.as_deref(), Some("sse"));
    }

    #[test]
    fn config_serializes_roundtrip() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn empty_toml_produces_defaults() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config, AppConfig::default());
    }
}
