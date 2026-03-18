use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::event::types::Action;
use crate::markdown;
use crate::ui::theme::Theme;

use super::Component;

/// Role of a message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    #[allow(dead_code)]
    System,
}

impl From<crate::llm::types::Role> for MessageRole {
    fn from(role: crate::llm::types::Role) -> Self {
        match role {
            crate::llm::types::Role::User => MessageRole::User,
            crate::llm::types::Role::Assistant => MessageRole::Assistant,
            crate::llm::types::Role::System => MessageRole::System,
            // Tool messages are displayed as system messages in the UI
            crate::llm::types::Role::Tool => MessageRole::System,
        }
    }
}

/// A single chat message for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: Option<chrono::DateTime<chrono::Local>>,
}

/// Center panel showing the conversation messages.
#[derive(Debug, Clone)]
pub struct ChatView {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) scroll_offset: u16,
    pub(crate) show_timestamps: bool,
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            show_timestamps: false,
        }
    }

    pub fn set_show_timestamps(&mut self, show: bool) {
        self.show_timestamps = show;
    }

    /// Build a ChatView from a slice of stored messages.
    pub fn from_messages(messages: &[crate::llm::types::Message]) -> Self {
        let mut view = Self::new();
        for msg in messages {
            view.add_message(ChatMessage {
                role: msg.role.clone().into(),
                content: msg.content.clone(),
                timestamp: None, // stored messages don't carry timestamps yet
            });
        }
        view
    }

    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.scroll_offset = 0;
    }

    pub fn append_to_last(&mut self, text: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(text);
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Build all lines for rendering, using markdown for assistant messages.
    fn build_lines(&self, theme: &Theme) -> Vec<Line<'_>> {
        let mut lines = Vec::new();

        for msg in &self.messages {
            let (label, color) = match msg.role {
                MessageRole::User => ("You", theme.user_label),
                MessageRole::Assistant => ("Assistant", theme.assistant_label),
                MessageRole::System => ("System", theme.system_label),
            };

            // Role label (optionally with timestamp)
            if self.show_timestamps {
                if let Some(ts) = &msg.timestamp {
                    let ts_str = ts.format("%H:%M:%S").to_string();
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{label}:"),
                            Style::default()
                                .fg(color)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("  {ts_str}"),
                            Style::default().fg(theme.timestamp),
                        ),
                    ]));
                } else {
                    lines.push(markdown::role_label(label, color));
                }
            } else {
                lines.push(markdown::role_label(label, color));
            }

            // Message content
            match msg.role {
                MessageRole::Assistant => {
                    // Render markdown for assistant responses
                    let rendered = markdown::render_markdown(&msg.content);
                    for line in rendered.lines {
                        lines.push(line.to_owned());
                    }
                }
                MessageRole::User | MessageRole::System => {
                    // Plain text for user and system messages
                    let rendered = markdown::render_plain(&msg.content);
                    for line in rendered.lines {
                        lines.push(line);
                    }
                }
            }

            // Separator between messages
            lines.push(Line::from(""));
            lines.push(markdown::separator(theme.separator));
            lines.push(Line::from(""));
        }

        lines
    }
}

impl Component for ChatView {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ScrollUp => {
                self.scroll_up();
                None
            }
            Action::ScrollDown => {
                self.scroll_down();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = super::focused_border_style(focused, theme);
        let lines = self.build_lines(theme);

        // Auto-scroll to bottom: calculate total lines vs visible area
        let inner_height = area.height.saturating_sub(2); // borders
        let total_lines = lines.len() as u16;
        let auto_scroll = if self.scroll_offset == 0 && total_lines > inner_height {
            total_lines.saturating_sub(inner_height)
        } else {
            self.scroll_offset
        };

        let text = Text::from(lines);
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Chat ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: false })
            .scroll((auto_scroll, 0));

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn chat_msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
            timestamp: None,
        }
    }

    #[test]
    fn new_chat_view_is_empty() {
        let view = ChatView::new();
        assert!(view.messages.is_empty());
        assert_eq!(view.scroll_offset, 0);
        assert!(!view.show_timestamps);
    }

    #[test]
    fn add_message_mutates_in_place() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].content, "Hello");
    }

    #[test]
    fn append_to_last_extends_last_message() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::Assistant, "Hello"));
        view.append_to_last(" world");
        assert_eq!(view.messages[0].content, "Hello world");
    }

    #[test]
    fn append_to_last_on_empty_is_noop() {
        let mut view = ChatView::new();
        view.append_to_last("text");
        assert!(view.messages.is_empty());
    }

    #[test]
    fn clear_removes_all_messages() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "hi"));
        view.clear();
        assert!(view.messages.is_empty());
    }

    #[test]
    fn scroll_up_increments_offset() {
        let mut view = ChatView::new();
        view.scroll_up();
        assert_eq!(view.scroll_offset, 1);
        view.scroll_up();
        assert_eq!(view.scroll_offset, 2);
    }

    #[test]
    fn scroll_down_decrements_offset() {
        let mut view = ChatView {
            messages: vec![],
            scroll_offset: 3,
            show_timestamps: false,
        };
        view.scroll_down();
        assert_eq!(view.scroll_offset, 2);
    }

    #[test]
    fn scroll_down_does_not_go_below_zero() {
        let mut view = ChatView::new();
        view.scroll_down();
        assert_eq!(view.scroll_offset, 0);
    }

    #[test]
    fn build_lines_includes_role_labels() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        view.add_message(chat_msg(MessageRole::Assistant, "Hi there"));
        let lines = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(content.contains("You:"));
        assert!(content.contains("Assistant:"));
        assert!(content.contains("Hello"));
        assert!(content.contains("Hi there"));
    }

    #[test]
    fn build_lines_renders_assistant_markdown() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::Assistant, "**bold text**"));
        let lines = view.build_lines(&theme);

        let has_bold = lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content.contains("bold text")
                    && span.style.add_modifier.contains(ratatui::style::Modifier::BOLD)
            })
        });
        assert!(has_bold, "Expected bold styling from markdown rendering");
    }

    #[test]
    fn build_lines_renders_user_as_plain_text() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "**not bold**"));
        let lines = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("**not bold**"));
    }

    #[test]
    fn build_lines_renders_code_blocks() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(
            MessageRole::Assistant,
            "```rust\nfn main() {}\n```",
        ));
        let lines = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            content.contains("fn") || content.contains("main"),
            "Expected code content in rendered output"
        );
    }

    #[test]
    fn build_lines_has_separators() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "hello"));
        view.add_message(chat_msg(MessageRole::Assistant, "hi"));
        let lines = view.build_lines(&theme);

        let has_separator = lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("\u{2500}"))
        });
        assert!(has_separator, "Expected separator between messages");
    }

    #[test]
    fn handle_action_scrolls() {
        let mut view = ChatView::new();
        view.handle_action(&Action::ScrollUp);
        assert_eq!(view.scroll_offset, 1);
        view.handle_action(&Action::ScrollDown);
        assert_eq!(view.scroll_offset, 0);
    }

    #[test]
    fn multiline_content_renders() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "line1\nline2\nline3"));
        let lines = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
    }

    #[test]
    fn timestamps_shown_when_enabled() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.set_show_timestamps(true);

        let ts = chrono::Local::now();
        view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "hello".to_string(),
            timestamp: Some(ts),
        });

        let lines = view.build_lines(&theme);
        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        // Should contain time in HH:MM:SS format
        let expected = ts.format("%H:%M:%S").to_string();
        assert!(
            content.contains(&expected),
            "Expected timestamp {expected} in output: {content}"
        );
    }

    #[test]
    fn timestamps_hidden_by_default() {
        let theme = test_theme();
        let mut view = ChatView::new();

        let ts = chrono::Local::now();
        view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "hello".to_string(),
            timestamp: Some(ts),
        });

        let lines = view.build_lines(&theme);
        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        let ts_str = ts.format("%H:%M:%S").to_string();
        assert!(
            !content.contains(&ts_str),
            "Timestamp should not appear when show_timestamps is false"
        );
    }

    #[test]
    fn set_show_timestamps_updates_flag() {
        let mut view = ChatView::new();
        assert!(!view.show_timestamps);
        view.set_show_timestamps(true);
        assert!(view.show_timestamps);
    }
}
