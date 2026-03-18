use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::event::types::Action;
use crate::markdown;
use crate::markdown::RenderOptions;
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

/// A match position in the conversation: (message index, byte offset in content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub message_idx: usize,
    pub byte_offset: usize,
}

/// Center panel showing the conversation messages.
#[derive(Debug, Clone)]
pub struct ChatView {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) scroll_offset: u16,
    pub(crate) show_timestamps: bool,
    pub(crate) render_options: RenderOptions,
    pub(crate) search_query: String,
    pub(crate) search_matches: Vec<SearchMatch>,
    pub(crate) search_current: usize,
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
            render_options: RenderOptions::default(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
        }
    }

    pub fn set_render_options(&mut self, opts: RenderOptions) {
        self.render_options = opts;
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

    /// Update the search query and recompute matches (case-insensitive).
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.recompute_matches();
    }

    /// Clear the search state.
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_matches.clear();
        self.search_current = 0;
    }

    /// Move to the next match. Returns the match info for status display.
    pub fn search_next(&mut self) -> Option<(usize, usize)> {
        if self.search_matches.is_empty() {
            return None;
        }
        self.search_current = (self.search_current + 1) % self.search_matches.len();
        Some((self.search_current + 1, self.search_matches.len()))
    }

    /// Move to the previous match. Returns the match info for status display.
    pub fn search_prev(&mut self) -> Option<(usize, usize)> {
        if self.search_matches.is_empty() {
            return None;
        }
        if self.search_current == 0 {
            self.search_current = self.search_matches.len() - 1;
        } else {
            self.search_current -= 1;
        }
        Some((self.search_current + 1, self.search_matches.len()))
    }

    /// Current search status: (current_match_1_indexed, total_matches).
    pub fn search_status(&self) -> Option<(usize, usize)> {
        if self.search_matches.is_empty() {
            return None;
        }
        Some((self.search_current + 1, self.search_matches.len()))
    }

    fn recompute_matches(&mut self) {
        self.search_matches.clear();
        self.search_current = 0;

        if self.search_query.is_empty() {
            return;
        }

        let query_lower = self.search_query.to_lowercase();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let content_lower = msg.content.to_lowercase();
            let mut start = 0;
            while let Some(pos) = content_lower[start..].find(&query_lower) {
                self.search_matches.push(SearchMatch {
                    message_idx: msg_idx,
                    byte_offset: start + pos,
                });
                start += pos + query_lower.len();
            }
        }
    }

    /// Build all lines for rendering, using markdown for assistant messages.
    /// Preprocesses assistant content to convert LaTeX and tables before rendering.
    fn build_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let highlight_style = Style::default()
            .bg(theme.highlight)
            .fg(ratatui::style::Color::Black)
            .add_modifier(Modifier::BOLD);
        let has_search = !self.search_query.is_empty();

        for (msg_idx, msg) in self.messages.iter().enumerate() {
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
                                .add_modifier(Modifier::BOLD),
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

            // Message content — assistant gets LaTeX/table preprocessing
            let content_lines: Vec<Line<'static>> = match msg.role {
                MessageRole::Assistant => {
                    markdown::render_markdown_preprocessed(&msg.content, self.render_options)
                }
                MessageRole::User | MessageRole::System => {
                    let rendered = markdown::render_plain(&msg.content);
                    rendered.lines.into_iter().collect()
                }
            };

            // Apply search highlighting if active
            if has_search && self.matches_in_message(msg_idx) {
                for line in content_lines {
                    lines.push(self.highlight_line(line, highlight_style));
                }
            } else {
                lines.extend(content_lines);
            }

            // Separator between messages
            lines.push(Line::from(""));
            lines.push(markdown::separator(theme.separator));
            lines.push(Line::from(""));
        }

        lines
    }

    /// Check if any search matches exist in a given message.
    fn matches_in_message(&self, msg_idx: usize) -> bool {
        self.search_matches.iter().any(|m| m.message_idx == msg_idx)
    }

    /// Highlight occurrences of the search query within a line's spans.
    fn highlight_line<'a>(&self, line: Line<'a>, highlight_style: Style) -> Line<'a> {
        if self.search_query.is_empty() {
            return line;
        }

        let query_lower = self.search_query.to_lowercase();
        let mut new_spans: Vec<Span<'a>> = Vec::new();

        for span in line.spans {
            let text = span.content.to_string();
            let text_lower = text.to_lowercase();

            if !text_lower.contains(&query_lower) {
                new_spans.push(span);
                continue;
            }

            let base_style = span.style;
            let mut pos = 0;
            while pos < text.len() {
                if let Some(match_pos) = text_lower[pos..].find(&query_lower) {
                    let abs_pos = pos + match_pos;
                    // Text before match
                    if abs_pos > pos {
                        new_spans.push(Span::styled(
                            text[pos..abs_pos].to_string(),
                            base_style,
                        ));
                    }
                    // The match itself
                    new_spans.push(Span::styled(
                        text[abs_pos..abs_pos + query_lower.len()].to_string(),
                        highlight_style,
                    ));
                    pos = abs_pos + query_lower.len();
                } else {
                    // Remainder
                    new_spans.push(Span::styled(text[pos..].to_string(), base_style));
                    break;
                }
            }
        }

        Line::from(new_spans)
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
        let mut view = ChatView::new();
        view.scroll_offset = 3;
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

    #[test]
    fn search_finds_matches_case_insensitive() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello World"));
        view.add_message(chat_msg(MessageRole::Assistant, "hello there"));
        view.set_search_query("hello".to_string());
        assert_eq!(view.search_matches.len(), 2);
    }

    #[test]
    fn search_no_matches() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        view.set_search_query("xyz".to_string());
        assert!(view.search_matches.is_empty());
        assert!(view.search_status().is_none());
    }

    #[test]
    fn search_next_cycles() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "foo foo foo"));
        view.set_search_query("foo".to_string());
        assert_eq!(view.search_matches.len(), 3);
        assert_eq!(view.search_status(), Some((1, 3)));

        assert_eq!(view.search_next(), Some((2, 3)));
        assert_eq!(view.search_next(), Some((3, 3)));
        assert_eq!(view.search_next(), Some((1, 3))); // wraps
    }

    #[test]
    fn search_prev_cycles() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "foo foo"));
        view.set_search_query("foo".to_string());
        assert_eq!(view.search_prev(), Some((2, 2))); // wraps to last
        assert_eq!(view.search_prev(), Some((1, 2)));
    }

    #[test]
    fn clear_search_resets_state() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "test"));
        view.set_search_query("test".to_string());
        assert_eq!(view.search_matches.len(), 1);
        view.clear_search();
        assert!(view.search_query.is_empty());
        assert!(view.search_matches.is_empty());
    }

    #[test]
    fn search_multiple_matches_per_message() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "ab ab ab"));
        view.set_search_query("ab".to_string());
        assert_eq!(view.search_matches.len(), 3);
        assert_eq!(view.search_matches[0].byte_offset, 0);
        assert_eq!(view.search_matches[1].byte_offset, 3);
        assert_eq!(view.search_matches[2].byte_offset, 6);
    }

    #[test]
    fn search_across_multiple_messages() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "hello world"));
        view.add_message(chat_msg(MessageRole::Assistant, "Hello again"));
        view.set_search_query("hello".to_string());
        assert_eq!(view.search_matches.len(), 2);
        assert_eq!(view.search_matches[0].message_idx, 0);
        assert_eq!(view.search_matches[1].message_idx, 1);
    }
}
