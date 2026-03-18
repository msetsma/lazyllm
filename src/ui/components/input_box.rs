use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::event::types::{Action, Mode};
use crate::ui::theme::Theme;

use super::Component;

/// Text input component for typing messages.
#[derive(Debug, Clone, Default)]
pub struct InputBox {
    pub(crate) content: String,
    pub(crate) cursor_pos: usize,
    pub(crate) mode: Mode,
}

impl InputBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.byte_position();
        self.content.insert(byte_pos, c);
        self.cursor_pos += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let new_cursor = self.cursor_pos - 1;
        let byte_pos = self.byte_position_at(new_cursor);
        let next_byte = self.byte_position();
        self.content.replace_range(byte_pos..next_byte, "");
        self.cursor_pos = new_cursor;
    }

    pub fn take_content(&mut self) -> String {
        let content = std::mem::take(&mut self.content);
        self.cursor_pos = 0;
        content
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Convert char position to byte position in the string.
    fn byte_position(&self) -> usize {
        self.byte_position_at(self.cursor_pos)
    }

    fn byte_position_at(&self, char_pos: usize) -> usize {
        self.content
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len())
    }
}

impl Component for InputBox {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::InsertChar(c) => {
                self.insert_char(*c);
                None
            }
            Action::DeleteChar => {
                self.delete_char();
                None
            }
            Action::SwitchMode(mode) => {
                self.set_mode(*mode);
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = super::focused_border_style(focused, theme);
        let mode_label = self.mode.label();

        let display_text = if self.mode == Mode::Command {
            format!(":{}", self.content)
        } else {
            self.content.clone()
        };

        let paragraph = Paragraph::new(display_text).block(
            Block::default()
                .title(format!(" Input [{mode_label}] "))
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        frame.render_widget(paragraph, area);

        // Show cursor when in insert or command mode
        if focused && (self.mode == Mode::Insert || self.mode == Mode::Command) {
            let cursor_offset = if self.mode == Mode::Command { 1 } else { 0 };
            let x = area.x + 1 + cursor_offset + self.cursor_pos as u16;
            let y = area.y + 1;
            if x < area.x + area.width - 1 {
                frame.set_cursor_position((x, y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_box_is_empty() {
        let input = InputBox::new();
        assert!(input.content.is_empty());
        assert_eq!(input.cursor_pos, 0);
        assert_eq!(input.mode, Mode::Normal);
    }

    #[test]
    fn insert_char_mutates_in_place() {
        let mut input = InputBox::new();
        input.insert_char('h');
        assert_eq!(input.content, "h");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn insert_multiple_chars() {
        let mut input = InputBox::new();
        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.content, "hi");
        assert_eq!(input.cursor_pos, 2);
    }

    #[test]
    fn delete_char_removes_last() {
        let mut input = InputBox::new();
        input.insert_char('a');
        input.insert_char('b');
        input.delete_char();
        assert_eq!(input.content, "a");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn delete_char_at_start_does_nothing() {
        let mut input = InputBox::new();
        input.delete_char();
        assert!(input.content.is_empty());
        assert_eq!(input.cursor_pos, 0);
    }

    #[test]
    fn take_content_clears_and_returns() {
        let mut input = InputBox::new();
        input.insert_char('h');
        input.insert_char('i');
        let content = input.take_content();
        assert_eq!(content, "hi");
        assert!(input.content.is_empty());
        assert_eq!(input.cursor_pos, 0);
    }

    #[test]
    fn set_mode_mutates_in_place() {
        let mut input = InputBox::new();
        input.set_mode(Mode::Insert);
        assert_eq!(input.mode, Mode::Insert);
    }

    #[test]
    fn handles_unicode_correctly() {
        let mut input = InputBox::new();
        input.insert_char('\u{1f980}');
        input.insert_char('!');
        input.delete_char();
        assert_eq!(input.content, "\u{1f980}");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn handle_action_insert_char() {
        let mut input = InputBox::new();
        input.handle_action(&Action::InsertChar('x'));
        assert_eq!(input.content, "x");
    }

    #[test]
    fn handle_action_delete_char() {
        let mut input = InputBox::new();
        input.handle_action(&Action::InsertChar('a'));
        input.handle_action(&Action::DeleteChar);
        assert!(input.content.is_empty());
    }

    #[test]
    fn handle_action_switch_mode() {
        let mut input = InputBox::new();
        input.handle_action(&Action::SwitchMode(Mode::Insert));
        assert_eq!(input.mode, Mode::Insert);
    }
}
