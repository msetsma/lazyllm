use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const VALID_PROVIDER_TYPES: &[&str] = &["openai", "anthropic", "ollama", "google"];

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: McpConfig,
}

impl AppConfig {
    /// Validates all config values, returning collected errors.
    /// Warnings (non-fatal) are emitted via `tracing::warn`.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // general.temperature: 0.0–2.0
        if let Some(t) = self.general.temperature {
            if !(0.0..=2.0).contains(&t) {
                errors.push(format!(
                    "general.temperature = {t} is out of range (must be 0.0–2.0)"
                ));
            }
        }

        // general.max_tokens: > 0
        if let Some(mt) = self.general.max_tokens {
            if mt == 0 {
                errors.push("general.max_tokens must be greater than 0".to_string());
            }
        }

        // ui.sidebar_width: 1–100
        if self.ui.sidebar_width == 0 || self.ui.sidebar_width > 100 {
            errors.push(format!(
                "ui.sidebar_width = {} is out of range (must be 1–100)",
                self.ui.sidebar_width
            ));
        }

        // ui.tool_panel_width: 1–100
        if self.ui.tool_panel_width == 0 || self.ui.tool_panel_width > 100 {
            errors.push(format!(
                "ui.tool_panel_width = {} is out of range (must be 1–100)",
                self.ui.tool_panel_width
            ));
        }

        // providers.*.provider_type
        for (name, provider) in &self.providers {
            if !VALID_PROVIDER_TYPES.contains(&provider.provider_type.as_str()) {
                errors.push(format!(
                    "providers.{name}.provider_type = \"{}\" is invalid (must be one of: {})",
                    provider.provider_type,
                    VALID_PROVIDER_TYPES.join(", ")
                ));
            }
        }

        // general.default_provider should match a providers key (warning only)
        if !self.providers.is_empty()
            && !self.providers.contains_key(&self.general.default_provider)
        {
            tracing::warn!(
                "general.default_provider = \"{}\" does not match any configured provider (available: {})",
                self.general.default_provider,
                self.providers.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Feature toggles for optional functionality.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeaturesConfig {
    /// Convert LaTeX math expressions to Unicode symbols.
    #[serde(default = "default_true")]
    pub latex_rendering: bool,
    /// Render markdown tables with box-drawing characters.
    #[serde(default = "default_true")]
    pub table_rendering: bool,
    /// Enable `/` search within conversations.
    #[serde(default = "default_true")]
    pub search: bool,
    /// Enable the context system (`:context` command).
    #[serde(default = "default_true")]
    pub contexts: bool,
    /// Enable MCP (Model Context Protocol) tool integration.
    #[serde(default = "default_true")]
    pub mcp_servers: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            latex_rendering: true,
            table_rendering: true,
            search: true,
            contexts: true,
            mcp_servers: true,
        }
    }
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
    /// Default temperature for LLM requests (0.0–2.0). None = provider default.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Default max tokens for LLM responses. None = provider default.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Global system prompt prepended to all conversations.
    #[serde(default)]
    pub system_prompt: Option<String>,
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
            temperature: None,
            max_tokens: None,
            system_prompt: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub show_tool_panel: bool,
    #[serde(default = "default_true")]
    pub show_sidebar: bool,
    #[serde(default)]
    pub show_timestamps: bool,
    #[serde(default = "default_true")]
    pub markdown_rendering: bool,
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
            show_sidebar: true,
            show_timestamps: false,
            markdown_rendering: true,
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
    /// Default model for this provider (overrides general.default_model when this provider is active).
    #[serde(default)]
    pub default_model: Option<String>,
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
        assert!(config.general.temperature.is_none());
        assert!(config.general.max_tokens.is_none());
        assert!(config.general.system_prompt.is_none());
        assert_eq!(config.ui.sidebar_width, 25);
        assert_eq!(config.ui.tool_panel_width, 20);
        assert!(config.ui.show_tool_panel);
        assert!(config.ui.show_sidebar);
        assert!(config.ui.markdown_rendering);
        assert!(!config.ui.show_timestamps);
        assert!(config.features.latex_rendering);
        assert!(config.features.table_rendering);
        assert!(config.features.search);
        assert!(config.features.contexts);
        assert!(config.features.mcp_servers);
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

    #[test]
    fn default_config_passes_validation() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn temperature_boundary_values() {
        let mut config = AppConfig::default();

        config.general.temperature = Some(0.0);
        assert!(config.validate().is_ok());

        config.general.temperature = Some(2.0);
        assert!(config.validate().is_ok());

        config.general.temperature = Some(-0.1);
        assert!(config.validate().is_err());

        config.general.temperature = Some(2.1);
        let errs = config.validate().unwrap_err();
        assert!(errs[0].contains("temperature"));
    }

    #[test]
    fn max_tokens_zero_is_error() {
        let mut config = AppConfig::default();
        config.general.max_tokens = Some(0);
        let errs = config.validate().unwrap_err();
        assert!(errs[0].contains("max_tokens"));
    }

    #[test]
    fn max_tokens_positive_is_ok() {
        let mut config = AppConfig::default();
        config.general.max_tokens = Some(1);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn sidebar_width_out_of_range() {
        let mut config = AppConfig::default();

        config.ui.sidebar_width = 0;
        assert!(config.validate().is_err());

        config.ui.sidebar_width = 101;
        assert!(config.validate().is_err());

        config.ui.sidebar_width = 1;
        assert!(config.validate().is_ok());

        config.ui.sidebar_width = 100;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn tool_panel_width_out_of_range() {
        let mut config = AppConfig::default();

        config.ui.tool_panel_width = 0;
        assert!(config.validate().is_err());

        config.ui.tool_panel_width = 101;
        assert!(config.validate().is_err());

        config.ui.tool_panel_width = 50;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_provider_type_is_error() {
        let mut config = AppConfig::default();
        config.providers.insert(
            "bad".to_string(),
            ProviderConfig {
                provider_type: "azure".to_string(),
                api_key_env: None,
                base_url: None,
                models: vec![],
                default_model: None,
            },
        );
        let errs = config.validate().unwrap_err();
        assert!(errs[0].contains("provider_type"));
        assert!(errs[0].contains("azure"));
    }

    #[test]
    fn valid_provider_types_accepted() {
        for pt in &["openai", "anthropic", "ollama", "google"] {
            let mut config = AppConfig::default();
            config.providers.insert(
                pt.to_string(),
                ProviderConfig {
                    provider_type: pt.to_string(),
                    api_key_env: None,
                    base_url: None,
                    models: vec![],
                    default_model: None,
                },
            );
            assert!(config.validate().is_ok(), "provider_type '{pt}' should be valid");
        }
    }

    #[test]
    fn multiple_errors_collected() {
        let mut config = AppConfig::default();
        config.general.temperature = Some(5.0);
        config.general.max_tokens = Some(0);
        config.ui.sidebar_width = 0;

        let errs = config.validate().unwrap_err();
        assert_eq!(errs.len(), 3);
    }
}
