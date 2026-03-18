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
    fn parse_unknown() {
        assert_eq!(
            parse_command("foobar"),
            Command::Unknown("foobar".to_string())
        );
    }

    #[test]
    fn parse_empty_input() {
        assert_eq!(parse_command(""), Command::Unknown("".to_string()));
        assert_eq!(parse_command("  "), Command::Unknown("".to_string()));
    }
}
