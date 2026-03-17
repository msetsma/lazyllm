pub mod types;

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result};

use types::AppConfig;

/// Returns the default config file path: ~/.config/lazyllm/config.toml
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("lazyllm")
        .join("config.toml")
}

/// Loads configuration from the given path, falling back to defaults
/// if the file doesn't exist.
pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let contents =
        std::fs::read_to_string(path).wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    let config: AppConfig =
        toml::from_str(&contents).wrap_err_with(|| format!("Failed to parse {}", path.display()))?;

    Ok(config)
}

/// Saves configuration to the given path, creating parent directories
/// if needed.
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

    #[test]
    fn load_config_returns_defaults_for_missing_file() {
        let result = load_config(Path::new("/nonexistent/path/config.toml")).unwrap();
        assert_eq!(result, AppConfig::default());
    }

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

    #[test]
    fn save_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("deep").join("config.toml");

        save_config(&AppConfig::default(), &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn load_config_errors_on_invalid_toml() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.toml");
        std::fs::write(&path, "this is not valid {{{{ toml").unwrap();

        let result = load_config(&path);
        assert!(result.is_err());
    }
}
