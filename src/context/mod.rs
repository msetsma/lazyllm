pub mod types;

use std::collections::HashMap;
use std::path::Path;

pub use types::Context;

/// Load all context TOML files from the given directory.
/// Returns a map from context name to Context.
pub fn load_contexts(dir: &Path) -> HashMap<String, Context> {
    let mut contexts = HashMap::new();

    if !dir.exists() {
        return contexts;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read contexts dir: {e}");
            return contexts;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            match load_context_file(&path) {
                Ok(ctx) => {
                    tracing::info!("Loaded context: {} ({})", ctx.name, path.display());
                    contexts.insert(ctx.name.clone(), ctx);
                }
                Err(e) => {
                    tracing::warn!("Failed to load context {}: {e}", path.display());
                }
            }
        }
    }

    contexts
}

fn load_context_file(path: &Path) -> Result<Context, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let ctx: Context = toml::from_str(&contents)?;
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_contexts_from_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let contexts = load_contexts(dir.path());
        assert!(contexts.is_empty());
    }

    #[test]
    fn load_contexts_from_nonexistent_dir() {
        let contexts = load_contexts(std::path::Path::new("/nonexistent/path"));
        assert!(contexts.is_empty());
    }

    #[test]
    fn load_contexts_finds_toml_files() {
        let dir = tempfile::tempdir().unwrap();
        let toml_content = r#"
name = "test-context"
description = "A test context"
system_prompt = "You are a test."
"#;
        std::fs::write(dir.path().join("test.toml"), toml_content).unwrap();
        // Non-toml files should be ignored
        std::fs::write(dir.path().join("readme.md"), "# not a context").unwrap();

        let contexts = load_contexts(dir.path());
        assert_eq!(contexts.len(), 1);
        assert!(contexts.contains_key("test-context"));
        assert_eq!(contexts["test-context"].system_prompt, "You are a test.");
    }

    #[test]
    fn load_contexts_skips_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.toml"), "not valid { toml").unwrap();
        std::fs::write(
            dir.path().join("good.toml"),
            r#"name = "good"
system_prompt = "hello"
"#,
        )
        .unwrap();

        let contexts = load_contexts(dir.path());
        assert_eq!(contexts.len(), 1);
        assert!(contexts.contains_key("good"));
    }
}
