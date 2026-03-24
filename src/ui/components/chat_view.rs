use std::cell::Cell;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

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
    pub(crate) scroll_offset: Cell<u16>,
    pub(crate) show_timestamps: bool,
    pub(crate) render_options: RenderOptions,
    pub(crate) markdown_rendering: bool,
    pub(crate) search_query: String,
    pub(crate) search_matches: Vec<SearchMatch>,
    pub(crate) search_current: usize,
    pub(crate) visual_mode: bool,
    pub(crate) visual_selection: Option<usize>,
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
            scroll_offset: Cell::new(0),
            show_timestamps: false,
            render_options: RenderOptions::default(),
            markdown_rendering: true,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
            visual_mode: false,
            visual_selection: None,
        }
    }

    pub fn set_markdown_rendering(&mut self, enabled: bool) {
        self.markdown_rendering = enabled;
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
        self.scroll_offset.set(0);
    }

    pub fn append_to_last(&mut self, text: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(text);
            self.scroll_offset.set(0);
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset.set(0);
    }

    fn scroll_up(&mut self) {
        self.scroll_offset.set(self.scroll_offset.get().saturating_add(1));
    }

    fn scroll_down(&mut self) {
        self.scroll_offset.set(self.scroll_offset.get().saturating_sub(1));
    }

    pub fn enter_visual_mode(&mut self) {
        self.visual_mode = true;
        // Start selection on the last message
        if !self.messages.is_empty() {
            self.visual_selection = Some(self.messages.len() - 1);
        }
    }

    pub fn exit_visual_mode(&mut self) {
        self.visual_mode = false;
        self.visual_selection = None;
        self.scroll_offset.set(0);
    }

    fn visual_select_prev(&mut self) {
        if let Some(idx) = self.visual_selection {
            if idx > 0 {
                self.visual_selection = Some(idx - 1);
            }
        }
    }

    fn visual_select_next(&mut self) {
        if let Some(idx) = self.visual_selection {
            if idx + 1 < self.messages.len() {
                self.visual_selection = Some(idx + 1);
            }
        }
    }

    /// Get the index of the currently selected message (or the last message).
    pub fn selected_message_index(&self) -> usize {
        self.visual_selection
            .unwrap_or_else(|| self.messages.len().saturating_sub(1))
    }

    /// Get the content of the currently selected message in visual mode.
    pub fn selected_content(&self) -> Option<&str> {
        self.visual_selection
            .and_then(|idx| self.messages.get(idx))
            .map(|m| m.content.as_str())
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
    /// Returns (lines, message_line_starts) where message_line_starts[i] is the
    /// logical line index where message i begins.
    #[cfg(test)]
    fn build_lines(&self, theme: &Theme) -> (Vec<Line<'static>>, Vec<usize>) {
        self.build_lines_with_width(theme, 0)
    }

    fn build_lines_with_width(
        &self,
        theme: &Theme,
        inner_width: usize,
    ) -> (Vec<Line<'static>>, Vec<usize>) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut message_line_starts: Vec<usize> = Vec::new();
        let highlight_style = Style::default()
            .bg(theme.highlight)
            .fg(ratatui::style::Color::Black)
            .add_modifier(Modifier::BOLD);
        let has_search = !self.search_query.is_empty();

        // Highlight the selected message in visual mode
        let visual_target = if self.visual_mode {
            self.visual_selection
        } else {
            None
        };

        for (msg_idx, msg) in self.messages.iter().enumerate() {
            message_line_starts.push(lines.len());
            let (label_color, _msg_bg) = match msg.role {
                MessageRole::User => (theme.user_label, theme.user_msg_bg),
                MessageRole::Assistant => (theme.assistant_label, theme.assistant_msg_bg),
                MessageRole::System => (theme.system_label, theme.assistant_msg_bg),
            };

            // Timestamp line (only if enabled)
            if self.show_timestamps {
                if let Some(ts) = &msg.timestamp {
                    let ts_str = ts.format("%H:%M:%S").to_string();
                    lines.push(Line::from(Span::styled(
                        ts_str,
                        Style::default().fg(theme.timestamp),
                    )));
                }
            }

            // Message content — assistant gets markdown + LaTeX/table preprocessing (if enabled)
            let content_lines: Vec<Line<'static>> = match msg.role {
                MessageRole::Assistant if self.markdown_rendering => {
                    markdown::render_markdown_preprocessed(&msg.content, self.render_options)
                }
                MessageRole::User | MessageRole::System | MessageRole::Assistant => {
                    let rendered = markdown::render_plain(&msg.content);
                    rendered.lines.into_iter().collect()
                }
            };

            // Prepend colored bar to first line, apply message background
            let is_visual_target = visual_target == Some(msg_idx);

            // Visual selection: bright left border on every line
            let bar_color = if is_visual_target {
                theme.visual_select
            } else {
                label_color
            };

            let is_user = msg.role == MessageRole::User;
            // User messages: right-aligned bubble with background
            // Max bubble width is 2/3 of inner width; indent fills the rest
            let max_bubble = if is_user && inner_width > 20 {
                (inner_width * 2) / 3
            } else {
                inner_width
            };
            // Find the longest content line to size the bubble
            let longest_content: usize = if is_user {
                content_lines
                    .iter()
                    .map(|l| l.spans.iter().map(|s| s.content.len()).sum::<usize>())
                    .max()
                    .unwrap_or(0)
            } else {
                0
            };
            // Bubble width: content + 2 padding + 2 for bar chars, capped at max
            let bubble_content_width = longest_content.min(max_bubble.saturating_sub(4));
            let bubble_total = bubble_content_width + 4; // " " + content + " " + "▐"  + 1 bar
            let bubble_indent = if is_user && inner_width > bubble_total {
                inner_width - bubble_total
            } else {
                0
            };

            for (_i, line) in content_lines.into_iter().enumerate() {
                let styled_line = if has_search && self.matches_in_message(msg_idx) {
                    self.highlight_line(line, highlight_style)
                } else if is_user {
                    // User bubble: indent + background color + bar on right
                    let bg_style = Style::default()
                        .bg(theme.user_msg_bg)
                        .fg(ratatui::style::Color::White);
                    let bar_style = if is_visual_target {
                        Style::default().fg(theme.visual_select)
                    } else {
                        Style::default().fg(bar_color)
                    };
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    if bubble_indent > 0 {
                        spans.push(Span::raw(" ".repeat(bubble_indent)));
                    }
                    spans.push(Span::styled(" ", bg_style));
                    // Pad each line to the bubble width for consistent background
                    let line_len: usize =
                        line.spans.iter().map(|s| s.content.len()).sum();
                    let pad = bubble_content_width.saturating_sub(line_len);
                    if pad > 0 {
                        spans.push(Span::styled(" ".repeat(pad), bg_style));
                    }
                    for span in line.spans {
                        spans.push(Span::styled(span.content, bg_style.patch(span.style)));
                    }
                    spans.push(Span::styled(" \u{2590}", bar_style));
                    Line::from(spans)
                } else {
                    // Assistant/system: left-aligned with colored bar
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    spans.push(Span::styled(
                        "\u{258c} ",
                        Style::default().fg(bar_color),
                    ));
                    for span in line.spans {
                        spans.push(Span::styled(span.content, span.style));
                    }
                    Line::from(spans)
                };
                lines.push(styled_line);
            }

            // Separator between messages
            lines.push(Line::from(""));
            lines.push(markdown::separator(theme.separator));
            lines.push(Line::from(""));
        }

        (lines, message_line_starts)
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
                if self.visual_mode {
                    self.visual_select_prev();
                } else {
                    self.scroll_up();
                }
                None
            }
            Action::ScrollDown => {
                if self.visual_mode {
                    self.visual_select_next();
                } else {
                    self.scroll_down();
                }
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = super::focused_border_style(focused, theme);

        let inner_height = area.height.saturating_sub(2) as usize; // borders
        let inner_width = area.width.saturating_sub(2) as usize; // borders

        let (lines, msg_line_starts) = self.build_lines_with_width(theme, inner_width);

        // Compute cumulative visual line offsets per logical line
        let visual_offsets: Vec<usize> = if inner_width == 0 {
            (0..lines.len()).collect()
        } else {
            let mut offsets = Vec::with_capacity(lines.len());
            let mut cumulative = 0usize;
            for line in &lines {
                offsets.push(cumulative);
                let width: usize = line.spans.iter().map(|s| s.content.len()).sum();
                cumulative += 1.max(width.div_ceil(inner_width));
            }
            offsets
        };

        let visual_lines = if let Some(last_offset) = visual_offsets.last() {
            // last offset + visual height of the last logical line
            let last_width: usize = lines.last().map_or(0, |l| {
                l.spans.iter().map(|s| s.content.len()).sum()
            });
            let last_height = if inner_width == 0 {
                1
            } else {
                1.max(last_width.div_ceil(inner_width))
            };
            last_offset + last_height
        } else {
            0
        };

        let max_scroll = visual_lines.saturating_sub(inner_height);

        // In visual mode, auto-scroll so the selected message is visible
        if self.visual_mode {
            if let Some(sel_idx) = self.visual_selection {
                if let Some(&logical_start) = msg_line_starts.get(sel_idx) {
                    // Visual line where the selected message starts
                    let sel_visual_start = visual_offsets
                        .get(logical_start)
                        .copied()
                        .unwrap_or(0);
                    // Visual line where it ends (start of next msg, or end)
                    let sel_visual_end = msg_line_starts
                        .get(sel_idx + 1)
                        .and_then(|&ls| visual_offsets.get(ls).copied())
                        .unwrap_or(visual_lines);

                    // Convert current scroll_offset (from bottom) to from-top
                    let current_top = max_scroll.saturating_sub(self.scroll_offset.get() as usize);
                    let current_bottom = current_top + inner_height;

                    let new_top = if sel_visual_start < current_top {
                        // Selected message is above viewport — scroll up to show it
                        sel_visual_start
                    } else if sel_visual_end > current_bottom {
                        // Selected message is below viewport — scroll down, but
                        // never past the selection start to prevent oscillation
                        // when a message is taller than the viewport.
                        sel_visual_end
                            .saturating_sub(inner_height)
                            .min(sel_visual_start)
                    } else {
                        current_top
                    };

                    // Convert back to offset-from-bottom
                    self.scroll_offset.set(max_scroll.saturating_sub(new_top) as u16);
                }
            }
        }

        // scroll_offset is "visual lines up from the bottom":
        //   0 = pinned to bottom (auto-scroll)
        //   N = N lines above the bottom
        let scroll_from_top = max_scroll.saturating_sub(self.scroll_offset.get() as usize);

        // Dynamic title with scroll position indicator
        let title = if self.scroll_offset.get() > 0 && visual_lines > inner_height {
            let pct = ((scroll_from_top + inner_height) as f64 / visual_lines as f64 * 100.0)
                as u16;
            format!(" Chat [{pct}%] ")
        } else {
            " Chat ".to_string()
        };

        let text = Text::from(lines);
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll_from_top as u16, 0));

        frame.render_widget(paragraph, area);

        // Scrollbar (only when content overflows), rendered inside the border
        if visual_lines > inner_height {
            let scrollbar_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: area.height.saturating_sub(2),
            };
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll).position(scroll_from_top);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(" "))
                .thumb_symbol("▐");
            frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
        }
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

    /// Ensures a new chat view starts with no messages, zero scroll, and timestamps off.
    #[test]
    fn new_chat_view_is_empty() {
        let view = ChatView::new();
        assert!(view.messages.is_empty());
        assert_eq!(view.scroll_offset.get(), 0);
        assert!(!view.show_timestamps);
    }

    /// Verifies add_message appends a message and preserves its content.
    #[test]
    fn add_message_mutates_in_place() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].content, "Hello");
    }

    /// Ensures append_to_last concatenates text to the last message's content.
    #[test]
    fn append_to_last_extends_last_message() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::Assistant, "Hello"));
        view.append_to_last(" world");
        assert_eq!(view.messages[0].content, "Hello world");
    }

    /// Ensures append_to_last on an empty view is a no-op without panicking.
    #[test]
    fn append_to_last_on_empty_is_noop() {
        let mut view = ChatView::new();
        view.append_to_last("text");
        assert!(view.messages.is_empty());
    }

    /// Verifies clear removes all messages from the view.
    #[test]
    fn clear_removes_all_messages() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "hi"));
        view.clear();
        assert!(view.messages.is_empty());
    }

    /// Ensures scroll_up increments the scroll offset each time.
    #[test]
    fn scroll_up_increments_offset() {
        let mut view = ChatView::new();
        view.scroll_up();
        assert_eq!(view.scroll_offset.get(), 1);
        view.scroll_up();
        assert_eq!(view.scroll_offset.get(), 2);
    }

    /// Ensures scroll_down decrements the scroll offset.
    #[test]
    fn scroll_down_decrements_offset() {
        let mut view = ChatView::new();
        view.scroll_offset.set(3);
        view.scroll_down();
        assert_eq!(view.scroll_offset.get(), 2);
    }

    /// Ensures scroll_down clamps at zero and does not underflow.
    #[test]
    fn scroll_down_does_not_go_below_zero() {
        let mut view = ChatView::new();
        view.scroll_down();
        assert_eq!(view.scroll_offset.get(), 0);
    }

    /// Verifies build_lines renders role indicators and message content.
    #[test]
    fn build_lines_includes_role_indicators_and_content() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        view.add_message(chat_msg(MessageRole::Assistant, "Hi there"));
        let (lines, _) = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        // Role indicators use a colored bar (\u{258c})
        assert!(content.contains("\u{258c}"), "Expected role indicator bar");
        assert!(content.contains("Hello"));
        assert!(content.contains("Hi there"));
    }

    /// Ensures assistant messages are rendered with markdown styling (e.g. bold).
    #[test]
    fn build_lines_renders_assistant_markdown() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::Assistant, "**bold text**"));
        let (lines, _) = view.build_lines(&theme);

        let has_bold = lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content.contains("bold text")
                    && span.style.add_modifier.contains(ratatui::style::Modifier::BOLD)
            })
        });
        assert!(has_bold, "Expected bold styling from markdown rendering");
    }

    /// Ensures user messages are rendered as plain text without markdown processing.
    #[test]
    fn build_lines_renders_user_as_plain_text() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "**not bold**"));
        let (lines, _) = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("**not bold**"));
    }

    /// Verifies fenced code blocks in assistant messages render their content.
    #[test]
    fn build_lines_renders_code_blocks() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(
            MessageRole::Assistant,
            "```rust\nfn main() {}\n```",
        ));
        let (lines, _) = view.build_lines(&theme);

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

    /// Ensures horizontal separator lines (─) appear between messages.
    #[test]
    fn build_lines_has_separators() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "hello"));
        view.add_message(chat_msg(MessageRole::Assistant, "hi"));
        let (lines, _) = view.build_lines(&theme);

        let has_separator = lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("\u{2500}"))
        });
        assert!(has_separator, "Expected separator between messages");
    }

    /// Verifies handle_action dispatches ScrollUp/ScrollDown to the scroll methods.
    #[test]
    fn handle_action_scrolls() {
        let mut view = ChatView::new();
        view.handle_action(&Action::ScrollUp);
        assert_eq!(view.scroll_offset.get(), 1);
        view.handle_action(&Action::ScrollDown);
        assert_eq!(view.scroll_offset.get(), 0);
    }

    /// Ensures multiline message content renders all lines in the output.
    #[test]
    fn multiline_content_renders() {
        let theme = test_theme();
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "line1\nline2\nline3"));
        let (lines, _) = view.build_lines(&theme);

        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
    }

    /// Verifies timestamps appear in rendered output when show_timestamps is enabled.
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

        let (lines, _) = view.build_lines(&theme);
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

    /// Ensures timestamps do not appear in rendered output by default.
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

        let (lines, _) = view.build_lines(&theme);
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

    /// Verifies set_show_timestamps mutates the flag from false to true.
    #[test]
    fn set_show_timestamps_updates_flag() {
        let mut view = ChatView::new();
        assert!(!view.show_timestamps);
        view.set_show_timestamps(true);
        assert!(view.show_timestamps);
    }

    /// Ensures search finds matches across messages regardless of case.
    #[test]
    fn search_finds_matches_case_insensitive() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello World"));
        view.add_message(chat_msg(MessageRole::Assistant, "hello there"));
        view.set_search_query("hello".to_string());
        assert_eq!(view.search_matches.len(), 2);
    }

    /// Ensures a search query with no results produces empty matches and no status.
    #[test]
    fn search_no_matches() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "Hello"));
        view.set_search_query("xyz".to_string());
        assert!(view.search_matches.is_empty());
        assert!(view.search_status().is_none());
    }

    /// Verifies search_next cycles through matches and wraps back to the first.
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

    /// Verifies search_prev wraps to the last match when moving before the first.
    #[test]
    fn search_prev_cycles() {
        let mut view = ChatView::new();
        view.add_message(chat_msg(MessageRole::User, "foo foo"));
        view.set_search_query("foo".to_string());
        assert_eq!(view.search_prev(), Some((2, 2))); // wraps to last
        assert_eq!(view.search_prev(), Some((1, 2)));
    }

    /// Ensures clear_search empties the query and match list.
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

    /// Verifies search detects multiple occurrences within a single message with correct byte offsets.
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

    /// Ensures search tracks correct message indices when matches span multiple messages.
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
