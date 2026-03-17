use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::event::types::Action;
use crate::markdown;

use super::Component;

/// Role of a message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    #[allow(dead_code)]
    System,
}

/// A single chat message for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

/// Center panel showing the conversation messages.
#[derive(Debug, Clone)]
pub struct ChatView {
    pub messages: Vec<ChatMessage>,
    pub scroll_offset: u16,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
        }
    }

    pub fn add_message(&self, message: ChatMessage) -> Self {
        let mut messages = self.messages.clone();
        messages.push(message);
        Self {
            messages,
            scroll_offset: 0,
        }
    }

    pub fn append_to_last(&self, text: &str) -> Self {
        if self.messages.is_empty() {
            return self.clone();
        }
        let mut messages = self.messages.clone();
        let last = messages.last_mut().unwrap();
        last.content.push_str(text);
        Self {
            messages,
            scroll_offset: 0,
        }
    }

    pub fn clear(&self) -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
        }
    }

    fn scroll_up(&self) -> Self {
        Self {
            messages: self.messages.clone(),
            scroll_offset: self.scroll_offset.saturating_add(1),
        }
    }

    fn scroll_down(&self) -> Self {
        Self {
            messages: self.messages.clone(),
            scroll_offset: self.scroll_offset.saturating_sub(1),
        }
    }

    /// Build all lines for rendering, using markdown for assistant messages.
    fn build_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();

        for msg in &self.messages {
            let (label, color) = match msg.role {
                MessageRole::User => ("You", Color::Green),
                MessageRole::Assistant => ("Assistant", Color::Blue),
                MessageRole::System => ("System", Color::Yellow),
            };

            // Role label
            lines.push(markdown::role_label(label, color));

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
            lines.push(markdown::separator());
            lines.push(Line::from(""));
        }

        lines
    }
}

impl Component for ChatView {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ScrollUp => {
                *self = self.scroll_up();
                None
            }
            Action::ScrollDown => {
                *self = self.scroll_down();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let lines = self.build_lines();

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
                    .border_style(Style::default().fg(border_color)),
            )
            .wrap(Wrap { trim: false })
            .scroll((auto_scroll, 0));

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_chat_view_is_empty() {
        let view = ChatView::new();
        assert!(view.messages.is_empty());
        assert_eq!(view.scroll_offset, 0);
    }

    #[test]
    fn add_message_returns_new_view() {
        let view = ChatView::new();
        let updated = view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "Hello".to_string(),
        });
        assert_eq!(updated.messages.len(), 1);
        assert_eq!(updated.messages[0].content, "Hello");
        assert!(view.messages.is_empty());
    }

    #[test]
    fn append_to_last_extends_last_message() {
        let view = ChatView::new().add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "Hello".to_string(),
        });
        let updated = view.append_to_last(" world");
        assert_eq!(updated.messages[0].content, "Hello world");
    }

    #[test]
    fn append_to_last_on_empty_is_noop() {
        let view = ChatView::new();
        let updated = view.append_to_last("text");
        assert!(updated.messages.is_empty());
    }

    #[test]
    fn clear_removes_all_messages() {
        let view = ChatView::new()
            .add_message(ChatMessage {
                role: MessageRole::User,
                content: "hi".to_string(),
            })
            .clear();
        assert!(view.messages.is_empty());
    }

    #[test]
    fn scroll_up_increments_offset() {
        let view = ChatView::new();
        let scrolled = view.scroll_up();
        assert_eq!(scrolled.scroll_offset, 1);
        let scrolled2 = scrolled.scroll_up();
        assert_eq!(scrolled2.scroll_offset, 2);
    }

    #[test]
    fn scroll_down_decrements_offset() {
        let view = ChatView {
            messages: vec![],
            scroll_offset: 3,
        };
        let scrolled = view.scroll_down();
        assert_eq!(scrolled.scroll_offset, 2);
    }

    #[test]
    fn scroll_down_does_not_go_below_zero() {
        let view = ChatView::new();
        let scrolled = view.scroll_down();
        assert_eq!(scrolled.scroll_offset, 0);
    }

    #[test]
    fn build_lines_includes_role_labels() {
        let view = ChatView::new()
            .add_message(ChatMessage {
                role: MessageRole::User,
                content: "Hello".to_string(),
            })
            .add_message(ChatMessage {
                role: MessageRole::Assistant,
                content: "Hi there".to_string(),
            });
        let lines = view.build_lines();

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
        let view = ChatView::new().add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "**bold text**".to_string(),
        });
        let lines = view.build_lines();

        // Should have styled content from markdown rendering
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
        let view = ChatView::new().add_message(ChatMessage {
            role: MessageRole::User,
            content: "**not bold**".to_string(),
        });
        let lines = view.build_lines();

        // User messages should NOT have bold styling - rendered as plain text
        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("**not bold**"));
    }

    #[test]
    fn build_lines_renders_code_blocks() {
        let view = ChatView::new().add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "```rust\nfn main() {}\n```".to_string(),
        });
        let lines = view.build_lines();

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
        let view = ChatView::new()
            .add_message(ChatMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
            })
            .add_message(ChatMessage {
                role: MessageRole::Assistant,
                content: "hi".to_string(),
            });
        let lines = view.build_lines();

        let has_separator = lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("─"))
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
        let view = ChatView::new().add_message(ChatMessage {
            role: MessageRole::User,
            content: "line1\nline2\nline3".to_string(),
        });
        let lines = view.build_lines();

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
    }
}
