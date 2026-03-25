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
    pub features: FeaturesConfig,
    #[serde(default)]
    pub conversation: ConversationConfig,
    #[serde(default)]
    pub usage: UsageConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: McpConfig,
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

/// Conversation and context window management settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationConfig {
    /// Compaction strategy: "none", "truncation", "summarization".
    #[serde(default = "default_compaction_strategy")]
    pub compaction_strategy: String,
    /// Context usage fraction (0.0–1.0) that triggers compaction warning.
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: f64,
    /// Number of recent messages to always keep during compaction.
    #[serde(default = "default_recent_messages")]
    pub recent_messages: usize,
    /// Maximum checkpoints to retain per conversation.
    #[serde(default = "default_max_checkpoints")]
    pub max_checkpoints: usize,
    /// Budget fraction of context window to use (0.0–1.0).
    #[serde(default = "default_budget_fraction")]
    pub budget_fraction: f64,
    /// Compaction mode: "client", "server", or "auto" (default).
    /// "auto" uses server-side for Anthropic, client-side for everything else.
    #[serde(default = "default_compaction_mode")]
    pub compaction_mode: String,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            compaction_strategy: default_compaction_strategy(),
            compaction_threshold: default_compaction_threshold(),
            recent_messages: default_recent_messages(),
            max_checkpoints: default_max_checkpoints(),
            budget_fraction: default_budget_fraction(),
            compaction_mode: default_compaction_mode(),
        }
    }
}

fn default_compaction_strategy() -> String {
    "auto".to_string()
}

fn default_compaction_threshold() -> f64 {
    0.75
}

fn default_recent_messages() -> usize {
    20
}

fn default_max_checkpoints() -> usize {
    5
}

fn default_budget_fraction() -> f64 {
    0.80
}

fn default_compaction_mode() -> String {
    "auto".to_string()
}

/// Usage tracking and cost display settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageConfig {
    /// Show token usage in the status bar after each response.
    #[serde(default = "default_true")]
    pub show_token_usage: bool,
    /// Show cost in the status bar (requires pricing data for the model).
    #[serde(default = "default_true")]
    pub show_cost: bool,
    /// Show context window usage percentage in the status bar.
    #[serde(default = "default_true")]
    pub show_context_usage: bool,
    /// Cost threshold (USD) for per-turn warning.
    #[serde(default)]
    pub cost_warning_threshold: Option<f64>,
    /// Custom per-million-token pricing overrides, keyed by model ID.
    #[serde(default)]
    pub custom_pricing: HashMap<String, CustomPricing>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            show_token_usage: true,
            show_cost: true,
            show_context_usage: true,
            cost_warning_threshold: None,
            custom_pricing: HashMap::new(),
        }
    }
}

/// Custom per-million-token pricing for a model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomPricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
    #[serde(default)]
    pub cache_read_per_million: f64,
    #[serde(default)]
    pub cache_write_per_million: f64,
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

    /// Verifies the default AppConfig has expected provider, model, UI, and feature defaults.
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

    /// Ensures a minimal TOML with only [general] fills in defaults for all other sections.
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

    /// Verifies provider configs with api_key_env, base_url, and models deserialize correctly.
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

    /// Verifies MCP server configs with name, command, args, and transport deserialize correctly.
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

    /// Ensures AppConfig survives a TOML serialize/deserialize roundtrip.
    #[test]
    fn config_serializes_roundtrip() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    /// Ensures an empty TOML string produces the same result as Default::default().
    #[test]
    fn empty_toml_produces_defaults() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config, AppConfig::default());
    }

    /// Verifies ConversationConfig defaults match expected compaction, threshold, and budget values.
    #[test]
    fn conversation_config_defaults() {
        let config = ConversationConfig::default();
        assert_eq!(config.compaction_strategy, "auto");
        assert!((config.compaction_threshold - 0.75).abs() < f64::EPSILON);
        assert_eq!(config.recent_messages, 20);
        assert_eq!(config.max_checkpoints, 5);
        assert!((config.budget_fraction - 0.80).abs() < f64::EPSILON);
    }

    /// Verifies UsageConfig defaults: display flags enabled, no warning threshold, no custom pricing.
    #[test]
    fn usage_config_defaults() {
        let config = UsageConfig::default();
        assert!(config.show_token_usage);
        assert!(config.show_cost);
        assert!(config.show_context_usage);
        assert!(config.cost_warning_threshold.is_none());
        assert!(config.custom_pricing.is_empty());
    }

    /// Ensures [conversation] TOML section overrides all ConversationConfig fields.
    #[test]
    fn conversation_config_from_toml() {
        let toml_str = r#"
[conversation]
compaction_strategy = "truncation"
recent_messages = 30
max_checkpoints = 10
budget_fraction = 0.90
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.conversation.compaction_strategy, "truncation");
        assert_eq!(config.conversation.recent_messages, 30);
        assert_eq!(config.conversation.max_checkpoints, 10);
        assert!((config.conversation.budget_fraction - 0.90).abs() < f64::EPSILON);
    }

    /// Ensures [usage] TOML section with custom pricing deserializes correctly.
    #[test]
    fn usage_config_from_toml() {
        let toml_str = r#"
[usage]
show_cost = false
cost_warning_threshold = 0.50

[usage.custom_pricing.my-model]
input_per_million = 1.0
output_per_million = 2.0
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.usage.show_cost);
        assert_eq!(config.usage.cost_warning_threshold, Some(0.50));
        assert!(config.usage.custom_pricing.contains_key("my-model"));
    }

    /// Verifies custom pricing with all fields survives a TOML roundtrip.
    #[test]
    fn custom_pricing_roundtrip() {
        let toml_str = r#"
[usage.custom_pricing.my-model]
input_per_million = 5.0
output_per_million = 15.0
cache_read_per_million = 0.5
cache_write_per_million = 2.0
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let pricing = &config.usage.custom_pricing["my-model"];
        assert!((pricing.input_per_million - 5.0).abs() < f64::EPSILON);
        assert!((pricing.output_per_million - 15.0).abs() < f64::EPSILON);
        assert!((pricing.cache_read_per_million - 0.5).abs() < f64::EPSILON);
        assert!((pricing.cache_write_per_million - 2.0).abs() < f64::EPSILON);
    }

    /// Ensures a config with all conversation and usage fields set deserializes and validates.
    #[test]
    fn config_with_all_new_sections() {
        let toml_str = r#"
[conversation]
compaction_strategy = "summarization"
recent_messages = 15
max_checkpoints = 3
budget_fraction = 0.70
compaction_threshold = 0.60

[usage]
show_token_usage = false
show_cost = true
show_context_usage = false
cost_warning_threshold = 1.0
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.conversation.compaction_strategy, "summarization");
        assert_eq!(config.conversation.recent_messages, 15);
        assert!(!config.usage.show_token_usage);
        assert!(!config.usage.show_context_usage);
        assert_eq!(config.usage.cost_warning_threshold, Some(1.0));
        assert!(config.validate().is_ok());
    }
}
