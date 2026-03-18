use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::llm::types::Message;

/// A file to include as context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextFile {
    pub path: PathBuf,
    pub label: String,
}

/// A named context bundle: system prompt + optional file references.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Context {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub files: Vec<ContextFile>,
}

impl Context {
    /// Build the system message for this context.
    /// Reads referenced files from disk; missing files produce a warning line.
    pub fn build_messages(&self) -> Vec<Message> {
        let mut content = self.system_prompt.clone();

        for file in &self.files {
            let expanded = expand_tilde(&file.path);
            match std::fs::read_to_string(&expanded) {
                Ok(text) => {
                    content.push_str(&format!(
                        "\n\n--- {} ({}) ---\n{}",
                        file.label,
                        expanded.display(),
                        text
                    ));
                }
                Err(e) => {
                    content.push_str(&format!(
                        "\n\n--- {} (MISSING: {}) ---",
                        file.label, e
                    ));
                }
            }
        }

        vec![Message::system(content)]
    }
}

/// Expand `~` prefix to the user's home directory.
fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_deserializes_from_toml() {
        let toml_str = r#"
name = "Rust Dev"
description = "Rust development assistant"
system_prompt = "You are a Rust expert."

[[files]]
path = "~/project/README.md"
label = "README"
"#;
        let ctx: Context = toml::from_str(toml_str).unwrap();
        assert_eq!(ctx.name, "Rust Dev");
        assert_eq!(ctx.description, "Rust development assistant");
        assert_eq!(ctx.system_prompt, "You are a Rust expert.");
        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.files[0].label, "README");
    }

    #[test]
    fn context_minimal_toml() {
        let toml_str = r#"name = "minimal""#;
        let ctx: Context = toml::from_str(toml_str).unwrap();
        assert_eq!(ctx.name, "minimal");
        assert!(ctx.description.is_empty());
        assert!(ctx.system_prompt.is_empty());
        assert!(ctx.files.is_empty());
    }

    #[test]
    fn build_messages_returns_system_message() {
        let ctx = Context {
            name: "test".to_string(),
            description: String::new(),
            system_prompt: "You are helpful.".to_string(),
            files: vec![],
        };
        let msgs = ctx.build_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, crate::llm::types::Role::System);
        assert_eq!(msgs[0].content, "You are helpful.");
    }

    #[test]
    fn build_messages_includes_missing_file_warning() {
        let ctx = Context {
            name: "test".to_string(),
            description: String::new(),
            system_prompt: "Base prompt.".to_string(),
            files: vec![ContextFile {
                path: PathBuf::from("/nonexistent/file.txt"),
                label: "Missing File".to_string(),
            }],
        };
        let msgs = ctx.build_messages();
        assert!(msgs[0].content.contains("MISSING:"));
        assert!(msgs[0].content.contains("Missing File"));
    }

    #[test]
    fn build_messages_reads_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "file contents here").unwrap();

        let ctx = Context {
            name: "test".to_string(),
            description: String::new(),
            system_prompt: "Base.".to_string(),
            files: vec![ContextFile {
                path: file_path,
                label: "Test File".to_string(),
            }],
        };
        let msgs = ctx.build_messages();
        assert!(msgs[0].content.contains("file contents here"));
        assert!(msgs[0].content.contains("Test File"));
    }

    #[test]
    fn expand_tilde_expands_home() {
        let path = PathBuf::from("~/some/file.txt");
        let expanded = expand_tilde(&path);
        assert!(!expanded.starts_with("~"));
        assert!(expanded.ends_with("some/file.txt"));
    }

    #[test]
    fn expand_tilde_leaves_absolute_paths() {
        let path = PathBuf::from("/absolute/path.txt");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, path);
    }

    #[test]
    fn context_serializes_roundtrip() {
        let ctx = Context {
            name: "test".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            files: vec![ContextFile {
                path: PathBuf::from("/tmp/test.md"),
                label: "Test".to_string(),
            }],
        };
        let toml_str = toml::to_string_pretty(&ctx).unwrap();
        let parsed: Context = toml::from_str(&toml_str).unwrap();
        assert_eq!(ctx, parsed);
    }
}
