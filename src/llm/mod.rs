pub mod anthropic;
pub mod capabilities;
pub mod compaction;
pub mod context;
pub mod google;
pub mod ollama;
pub mod openai;
pub mod pricing;
pub mod streaming;
pub mod types;

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::config::types::AppConfig;
use types::{ChatRequest, LlmError, ModelInfo, StreamChunk};

/// Trait for LLM provider implementations.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider display name (e.g., "OpenAI", "Anthropic").
    fn name(&self) -> &str;

    /// List of models available from this provider.
    fn available_models(&self) -> Vec<ModelInfo>;

    /// Send a chat request and stream the response through the channel.
    /// The provider should send `StreamChunk::Delta` for each text chunk,
    /// and `StreamChunk::Done` when complete.
    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<(), LlmError>;
}

/// Registry of available LLM providers, keyed by name.
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn LlmProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn LlmProvider>) {
        let name = provider.name().to_string();
        self.providers.insert(name, provider);
    }

    pub fn get(&self, name: &str) -> Option<&dyn LlmProvider> {
        self.providers.get(name).map(|p| p.as_ref())
    }

    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Build a provider registry from the application config.
pub fn build_registry(config: &AppConfig) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    for (name, provider_config) in &config.providers {
        let models: Vec<ModelInfo> = provider_config
            .models
            .iter()
            .map(ModelInfo::new)
            .collect();

        match provider_config.provider_type.as_str() {
            "ollama" => {
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("http://localhost:11434");
                let provider = ollama::OllamaProvider::new(name, base_url, models);
                registry.register(Box::new(provider));
                tracing::info!("Registered Ollama provider: {name}");
            }
            "anthropic" => {
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://api.anthropic.com/v1/messages");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = anthropic::AnthropicProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered Anthropic provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
            "google" => {
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://generativelanguage.googleapis.com/v1beta");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = google::GoogleProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered Google provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
            _ => {
                // Default: OpenAI-compatible
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = openai::OpenAiProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered OpenAI provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
        }
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::ModelInfo;

    struct MockProvider {
        name: String,
        models: Vec<ModelInfo>,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn available_models(&self) -> Vec<ModelInfo> {
            self.models.clone()
        }

        async fn chat(
            &self,
            _request: ChatRequest,
            tx: mpsc::UnboundedSender<StreamChunk>,
        ) -> Result<(), LlmError> {
            tx.send(StreamChunk::Delta("Hello".to_string())).ok();
            tx.send(StreamChunk::Done).ok();
            Ok(())
        }
    }

    fn mock_provider(name: &str) -> Box<dyn LlmProvider> {
        Box::new(MockProvider {
            name: name.to_string(),
            models: vec![ModelInfo::new("test-model")],
        })
    }

    /// Ensures a new registry starts with no providers.
    #[test]
    fn registry_starts_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.is_empty());
        assert!(registry.list_providers().is_empty());
    }

    /// Verifies providers can be registered and retrieved by name.
    #[test]
    fn register_and_get_provider() {
        let mut registry = ProviderRegistry::new();
        registry.register(mock_provider("openai"));

        assert!(!registry.is_empty());
        let provider = registry.get("openai").unwrap();
        assert_eq!(provider.name(), "openai");
    }

    /// Ensures lookup of a non-registered provider returns None.
    #[test]
    fn get_nonexistent_provider_returns_none() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    /// Verifies list_providers returns all registered provider names.
    #[test]
    fn list_providers_returns_all_names() {
        let mut registry = ProviderRegistry::new();
        registry.register(mock_provider("openai"));
        registry.register(mock_provider("anthropic"));

        let mut names = registry.list_providers();
        names.sort();
        assert_eq!(names, vec!["anthropic", "openai"]);
    }

    /// Verifies the LlmProvider trait's chat method streams Delta then Done via channel.
    #[tokio::test]
    async fn mock_provider_streams_response() {
        let provider = mock_provider("test");
        let (tx, mut rx) = mpsc::unbounded_channel();

        let request = ChatRequest::new("test-model", vec![]);
        provider.chat(request, tx).await.unwrap();

        let chunk1 = rx.recv().await.unwrap();
        assert_eq!(chunk1, StreamChunk::Delta("Hello".to_string()));

        let chunk2 = rx.recv().await.unwrap();
        assert_eq!(chunk2, StreamChunk::Done);
    }

    /// Ensures build_registry with an empty config produces an empty registry.
    #[test]
    fn build_registry_empty_config() {
        let config = AppConfig::default();
        let registry = build_registry(&config);
        assert!(registry.is_empty());
    }

    /// Ensures Ollama provider is registered without requiring an API key env var.
    #[test]
    fn build_registry_ollama_no_api_key_needed() {
        use crate::config::types::ProviderConfig;

        let mut config = AppConfig::default();
        config.providers.insert(
            "local-ollama".to_string(),
            ProviderConfig {
                provider_type: "ollama".to_string(),
                api_key_env: None,
                base_url: Some("http://localhost:11434".to_string()),
                models: vec!["llama3".to_string()],
                default_model: None,
            },
        );

        let registry = build_registry(&config);
        assert!(!registry.is_empty());
        assert!(registry.get("local-ollama").is_some());
    }

    /// Ensures an OpenAI-type provider is skipped when its API key env var is not set.
    #[test]
    fn build_registry_skips_provider_without_api_key() {
        use crate::config::types::ProviderConfig;

        let mut config = AppConfig::default();
        config.providers.insert(
            "my-openai".to_string(),
            ProviderConfig {
                provider_type: "openai".to_string(),
                api_key_env: Some("LAZYLLM_TEST_MISSING_KEY_XYZ".to_string()),
                base_url: None,
                models: vec!["gpt-4o".to_string()],
                default_model: None,
            },
        );

        // Ensure the env var is not set
        unsafe { std::env::remove_var("LAZYLLM_TEST_MISSING_KEY_XYZ") };

        let registry = build_registry(&config);
        assert!(registry.get("my-openai").is_none());
    }

    /// Ensures an OpenAI-type provider is registered when its API key env var is set.
    #[test]
    fn build_registry_registers_provider_with_api_key() {
        use crate::config::types::ProviderConfig;

        let mut config = AppConfig::default();
        config.providers.insert(
            "test-openai".to_string(),
            ProviderConfig {
                provider_type: "openai".to_string(),
                api_key_env: Some("LAZYLLM_TEST_OPENAI_KEY".to_string()),
                base_url: None,
                models: vec!["gpt-4o".to_string()],
                default_model: None,
            },
        );

        unsafe { std::env::set_var("LAZYLLM_TEST_OPENAI_KEY", "sk-test-dummy") };
        let registry = build_registry(&config);
        unsafe { std::env::remove_var("LAZYLLM_TEST_OPENAI_KEY") };

        assert!(registry.get("test-openai").is_some());
        assert_eq!(registry.get("test-openai").unwrap().name(), "test-openai");
    }

    /// Ensures multiple provider types can be registered in a single config.
    #[test]
    fn build_registry_multiple_providers() {
        use crate::config::types::ProviderConfig;

        let mut config = AppConfig::default();
        config.providers.insert(
            "ollama".to_string(),
            ProviderConfig {
                provider_type: "ollama".to_string(),
                api_key_env: None,
                base_url: None,
                models: vec!["llama3".to_string()],
                default_model: None,
            },
        );
        config.providers.insert(
            "my-anthropic".to_string(),
            ProviderConfig {
                provider_type: "anthropic".to_string(),
                api_key_env: Some("LAZYLLM_TEST_ANTHROPIC_KEY".to_string()),
                base_url: None,
                models: vec!["claude-3".to_string()],
                default_model: None,
            },
        );

        unsafe { std::env::set_var("LAZYLLM_TEST_ANTHROPIC_KEY", "sk-ant-test") };
        let registry = build_registry(&config);
        unsafe { std::env::remove_var("LAZYLLM_TEST_ANTHROPIC_KEY") };

        assert!(registry.get("ollama").is_some());
        assert!(registry.get("my-anthropic").is_some());
        assert_eq!(registry.list_providers().len(), 2);
    }
}
