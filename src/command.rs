/// Parsed command from `:` command mode input.
#[derive(Debug, PartialEq)]
pub enum Command {
    Quit,
    Model(String),
    Provider(String),
    NewChat,
    DeleteChat,
    Help,
    Clear,
    /// Set or clear the active context. None = clear, Some(name) = activate.
    Context(Option<String>),
    /// Export the active conversation to a JSON file.
    Export,
    /// Import a conversation from a JSON file path.
    Import(String),
    /// Show usage stats for the current conversation.
    Usage,
    /// Show cost report across conversations.
    Spend,
    /// Manually compact the current conversation.
    Compact,
    /// List checkpoints for the current conversation.
    Checkpoints,
    /// Restore a checkpoint by index.
    Restore(Option<i64>),
    /// Set a runtime configuration value.
    Set(String, String),
    Unknown(String),
}

/// Parse a command mode input string into a `Command`.
pub fn parse_command(input: &str) -> Command {
    let trimmed = input.trim();
    let (cmd, arg) = match trimmed.split_once(char::is_whitespace) {
        Some((c, a)) => (c, Some(a.trim())),
        None => (trimmed, None),
    };

    match cmd {
        "q" | "quit" => Command::Quit,
        "model" => match arg {
            Some(a) if !a.is_empty() => Command::Model(a.to_string()),
            _ => Command::Unknown("model requires an argument".to_string()),
        },
        "provider" => match arg {
            Some(a) if !a.is_empty() => Command::Provider(a.to_string()),
            _ => Command::Unknown("provider requires an argument".to_string()),
        },
        "new" => Command::NewChat,
        "delete" | "del" => Command::DeleteChat,
        "help" => Command::Help,
        "clear" => Command::Clear,
        "context" | "ctx" => match arg {
            Some(a) if a == "none" || a == "clear" => Command::Context(None),
            Some(a) if !a.is_empty() => Command::Context(Some(a.to_string())),
            _ => Command::Context(None),
        },
        "export" => Command::Export,
        "import" => match arg {
            Some(a) if !a.is_empty() => Command::Import(a.to_string()),
            _ => Command::Unknown("import requires a file path".to_string()),
        },
        "usage" | "tokens" => Command::Usage,
        "spend" | "cost" => Command::Spend,
        "compact" => Command::Compact,
        "checkpoints" => Command::Checkpoints,
        "restore" => match arg {
            Some(a) if !a.is_empty() => match a.parse::<i64>() {
                Ok(id) => Command::Restore(Some(id)),
                Err(_) => Command::Unknown("restore requires a numeric checkpoint ID".to_string()),
            },
            _ => Command::Restore(None), // restore latest
        },
        "set" => match arg {
            Some(a) if !a.is_empty() => {
                if let Some((key, value)) = a.split_once(char::is_whitespace) {
                    Command::Set(key.trim().to_string(), value.trim().to_string())
                } else {
                    Command::Unknown("set requires key and value (e.g. :set temperature 0.7)".to_string())
                }
            }
            _ => Command::Unknown("set requires key and value".to_string()),
        },
        other => Command::Unknown(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quit() {
        assert_eq!(parse_command("q"), Command::Quit);
        assert_eq!(parse_command("quit"), Command::Quit);
        assert_eq!(parse_command("  quit  "), Command::Quit);
    }

    #[test]
    fn parse_model() {
        assert_eq!(
            parse_command("model gpt-4o"),
            Command::Model("gpt-4o".to_string())
        );
        assert_eq!(
            parse_command("model  claude-sonnet-4-20250514"),
            Command::Model("claude-sonnet-4-20250514".to_string())
        );
    }

    #[test]
    fn parse_model_without_arg() {
        assert!(matches!(parse_command("model"), Command::Unknown(_)));
        assert!(matches!(parse_command("model  "), Command::Unknown(_)));
    }

    #[test]
    fn parse_provider() {
        assert_eq!(
            parse_command("provider openai"),
            Command::Provider("openai".to_string())
        );
    }

    #[test]
    fn parse_provider_without_arg() {
        assert!(matches!(parse_command("provider"), Command::Unknown(_)));
    }

    #[test]
    fn parse_new_chat() {
        assert_eq!(parse_command("new"), Command::NewChat);
    }

    #[test]
    fn parse_delete_chat() {
        assert_eq!(parse_command("delete"), Command::DeleteChat);
        assert_eq!(parse_command("del"), Command::DeleteChat);
    }

    #[test]
    fn parse_help() {
        assert_eq!(parse_command("help"), Command::Help);
    }

    #[test]
    fn parse_clear() {
        assert_eq!(parse_command("clear"), Command::Clear);
    }

    #[test]
    fn parse_context_with_name() {
        assert_eq!(
            parse_command("context rust-dev"),
            Command::Context(Some("rust-dev".to_string()))
        );
        assert_eq!(
            parse_command("ctx rust-dev"),
            Command::Context(Some("rust-dev".to_string()))
        );
    }

    #[test]
    fn parse_context_clear() {
        assert_eq!(parse_command("context none"), Command::Context(None));
        assert_eq!(parse_command("context clear"), Command::Context(None));
        assert_eq!(parse_command("context"), Command::Context(None));
        assert_eq!(parse_command("ctx"), Command::Context(None));
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(
            parse_command("foobar"),
            Command::Unknown("foobar".to_string())
        );
    }

    #[test]
    fn parse_export() {
        assert_eq!(parse_command("export"), Command::Export);
    }

    #[test]
    fn parse_import() {
        assert_eq!(
            parse_command("import /tmp/chat.json"),
            Command::Import("/tmp/chat.json".to_string())
        );
    }

    #[test]
    fn parse_import_without_arg() {
        assert!(matches!(parse_command("import"), Command::Unknown(_)));
    }

    #[test]
    fn parse_usage() {
        assert_eq!(parse_command("usage"), Command::Usage);
        assert_eq!(parse_command("tokens"), Command::Usage);
    }

    #[test]
    fn parse_spend() {
        assert_eq!(parse_command("spend"), Command::Spend);
        assert_eq!(parse_command("cost"), Command::Spend);
    }

    #[test]
    fn parse_compact() {
        assert_eq!(parse_command("compact"), Command::Compact);
    }

    #[test]
    fn parse_checkpoints() {
        assert_eq!(parse_command("checkpoints"), Command::Checkpoints);
    }

    #[test]
    fn parse_set() {
        assert_eq!(
            parse_command("set temperature 0.7"),
            Command::Set("temperature".to_string(), "0.7".to_string())
        );
        assert!(matches!(parse_command("set"), Command::Unknown(_)));
        assert!(matches!(parse_command("set temperature"), Command::Unknown(_)));
    }

    #[test]
    fn parse_restore() {
        assert_eq!(parse_command("restore"), Command::Restore(None));
        assert_eq!(parse_command("restore 3"), Command::Restore(Some(3)));
        assert!(matches!(parse_command("restore abc"), Command::Unknown(_)));
    }

    #[test]
    fn parse_empty_input() {
        assert_eq!(parse_command(""), Command::Unknown("".to_string()));
        assert_eq!(parse_command("  "), Command::Unknown("".to_string()));
    }
}
