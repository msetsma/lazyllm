pub mod types;

use std::path::{Path, PathBuf};

use color_eyre::eyre::{self, Context, Result};

use types::AppConfig;

/// The example config embedded at compile time, used to seed first-run config files.
const EXAMPLE_CONFIG: &str = include_str!("../../config.example.toml");

/// Returns the default config file path: ~/.config/lazyllm/config.toml
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("lazyllm")
        .join("config.toml")
}

/// Writes the example config to the given path if it does not already exist.
/// Failures are logged but not fatal — the app can run with in-memory defaults.
fn write_default_config(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Could not create config directory {}: {e}", parent.display());
            return;
        }
    }
    if let Err(e) = std::fs::write(path, EXAMPLE_CONFIG) {
        tracing::warn!("Could not write default config to {}: {e}", path.display());
    } else {
        tracing::info!("Created default config at {}", path.display());
    }
}

/// Loads configuration from the given path, falling back to defaults
/// if the file doesn't exist. On first run the example config is written
/// to disk so users have a documented starting point.
pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        write_default_config(path);
        return Ok(AppConfig::default());
    }

    let contents =
        std::fs::read_to_string(path).wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    let config: AppConfig =
        toml::from_str(&contents).wrap_err_with(|| format!("Failed to parse {}", path.display()))?;

    config.validate().map_err(|errors| {
        eyre::eyre!(
            "Config validation failed ({}): \n  - {}",
            path.display(),
            errors.join("\n  - ")
        )
    })?;

    Ok(config)
}

/// Saves configuration to the given path, creating parent directories
/// if needed.
#[allow(dead_code)] // TODO: will be used when config editing is added
pub fn save_config(config: &AppConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let contents = toml::to_string_pretty(config).wrap_err("Failed to serialize config")?;

    std::fs::write(path, contents)
        .wrap_err_with(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Ensures missing config file returns sensible defaults.
    #[test]
    fn load_config_returns_defaults_for_missing_file() {
        let result = load_config(Path::new("/nonexistent/path/config.toml")).unwrap();
        assert_eq!(result, AppConfig::default());
    }

    /// Validates that a saved config can be loaded back with identical values.
    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        let mut config = AppConfig::default();
        config.general.default_provider = "anthropic".to_string();
        config.general.default_model = "claude-sonnet-4-20250514".to_string();

        save_config(&config, &path).unwrap();
        let loaded = load_config(&path).unwrap();

        assert_eq!(loaded.general.default_provider, "anthropic");
        assert_eq!(loaded.general.default_model, "claude-sonnet-4-20250514");
    }

    /// Ensures save_config creates intermediate directories when they do not exist.
    #[test]
    fn save_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("deep").join("config.toml");

        save_config(&AppConfig::default(), &path).unwrap();
        assert!(path.exists());
    }

    /// Ensures malformed TOML content produces a parse error.
    #[test]
    fn load_config_errors_on_invalid_toml() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.toml");
        std::fs::write(&path, "this is not valid {{{{ toml").unwrap();

        let result = load_config(&path);
        assert!(result.is_err());
    }

    /// Validates that the embedded example config file parses to the same values as Default.
    #[test]
    fn example_config_parses_to_defaults() {
        let config: AppConfig = toml::from_str(EXAMPLE_CONFIG).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    /// Ensures first-run load writes the default config file to disk.
    #[test]
    fn load_config_writes_default_on_first_run() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("lazyllm").join("config.toml");

        let config = load_config(&path).unwrap();
        assert_eq!(config, AppConfig::default());
        assert!(path.exists(), "default config should have been written");
    }

    /// Validates that out-of-range values in a config file produce a validation error listing all violations.
    #[test]
    fn load_config_rejects_invalid_values() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[general]
temperature = 5.0
max_tokens = 0

[ui]
sidebar_width = 0
"#,
        )
        .unwrap();

        let result = load_config(&path);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("temperature"), "should mention temperature");
        assert!(msg.contains("max_tokens"), "should mention max_tokens");
        assert!(msg.contains("sidebar_width"), "should mention sidebar_width");
    }
}
