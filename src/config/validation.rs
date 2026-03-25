use super::types::AppConfig;

const VALID_PROVIDER_TYPES: &[&str] = &["openai", "anthropic", "ollama", "google"];

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

        // conversation.compaction_strategy
        let valid_strategies = ["auto", "none", "truncation", "summarization"];
        if !valid_strategies.contains(&self.conversation.compaction_strategy.as_str()) {
            errors.push(format!(
                "conversation.compaction_strategy = \"{}\" is invalid (must be one of: {})",
                self.conversation.compaction_strategy,
                valid_strategies.join(", ")
            ));
        }

        // conversation.compaction_mode
        let valid_modes = ["auto", "client", "server"];
        if !valid_modes.contains(&self.conversation.compaction_mode.as_str()) {
            errors.push(format!(
                "conversation.compaction_mode = \"{}\" is invalid (must be one of: {})",
                self.conversation.compaction_mode,
                valid_modes.join(", ")
            ));
        }

        // conversation.budget_fraction: 0.1–1.0
        if !(0.1..=1.0).contains(&self.conversation.budget_fraction) {
            errors.push(format!(
                "conversation.budget_fraction = {} is out of range (must be 0.1–1.0)",
                self.conversation.budget_fraction
            ));
        }

        // conversation.compaction_threshold: 0.1–1.0
        if !(0.1..=1.0).contains(&self.conversation.compaction_threshold) {
            errors.push(format!(
                "conversation.compaction_threshold = {} is out of range (must be 0.1–1.0)",
                self.conversation.compaction_threshold
            ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ProviderConfig;

    /// Ensures the default config passes validation without errors.
    #[test]
    fn default_config_passes_validation() {
        AppConfig::default().validate().unwrap();
    }

    /// Validates temperature accepts 0.0–2.0 and rejects values outside that range.
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

    /// Ensures max_tokens = 0 is rejected by validation.
    #[test]
    fn max_tokens_zero_is_error() {
        let mut config = AppConfig::default();
        config.general.max_tokens = Some(0);
        let errs = config.validate().unwrap_err();
        assert!(errs[0].contains("max_tokens"));
    }

    /// Ensures max_tokens > 0 passes validation.
    #[test]
    fn max_tokens_positive_is_ok() {
        let mut config = AppConfig::default();
        config.general.max_tokens = Some(1);
        assert!(config.validate().is_ok());
    }

    /// Validates sidebar_width accepts 1–100 and rejects 0 and >100.
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

    /// Validates tool_panel_width accepts 1–100 and rejects 0 and >100.
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

    /// Ensures an unsupported provider_type (e.g. "azure") is rejected by validation.
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

    /// Ensures all supported provider types (openai, anthropic, ollama, google) pass validation.
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

    /// Verifies validation collects all errors instead of stopping at the first one.
    #[test]
    fn multiple_errors_collected() {
        let mut config = AppConfig::default();
        config.general.temperature = Some(5.0);
        config.general.max_tokens = Some(0);
        config.ui.sidebar_width = 0;

        let errs = config.validate().unwrap_err();
        assert_eq!(errs.len(), 3);
    }

    /// Ensures an invalid compaction_strategy is rejected by validation.
    #[test]
    fn invalid_compaction_strategy_is_error() {
        let mut config = AppConfig::default();
        config.conversation.compaction_strategy = "invalid".to_string();
        let errs = config.validate().unwrap_err();
        assert!(errs[0].contains("compaction_strategy"));
    }

    /// Validates budget_fraction rejects values below 0.1 and above 1.0.
    #[test]
    fn budget_fraction_out_of_range() {
        let mut config = AppConfig::default();
        config.conversation.budget_fraction = 0.05;
        assert!(config.validate().is_err());
        config.conversation.budget_fraction = 1.5;
        assert!(config.validate().is_err());
        config.conversation.budget_fraction = 0.5;
        assert!(config.validate().is_ok());
    }

    /// Validates compaction_threshold rejects values below 0.1 and above 1.0.
    #[test]
    fn compaction_threshold_out_of_range() {
        let mut config = AppConfig::default();
        config.conversation.compaction_threshold = 0.05;
        assert!(config.validate().is_err());
        config.conversation.compaction_threshold = 1.5;
        assert!(config.validate().is_err());
        config.conversation.compaction_threshold = 0.75;
        assert!(config.validate().is_ok());
    }

    /// Ensures all supported compaction strategies (auto, none, truncation, summarization) pass.
    #[test]
    fn valid_compaction_strategies_accepted() {
        for strategy in &["auto", "none", "truncation", "summarization"] {
            let mut config = AppConfig::default();
            config.conversation.compaction_strategy = strategy.to_string();
            assert!(config.validate().is_ok(), "strategy '{strategy}' should be valid");
        }
    }
}
