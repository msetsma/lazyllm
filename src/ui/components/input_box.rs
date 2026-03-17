use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::event::types::{Action, Mode};

use super::Component;

/// Text input component for typing messages.
#[derive(Debug, Clone, Default)]
pub struct InputBox {
    pub content: String,
    pub cursor_pos: usize,
    pub mode: Mode,
}

impl InputBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_char(&self, c: char) -> Self {
        let mut content = self.content.clone();
        let byte_pos = self.byte_position();
        content.insert(byte_pos, c);
        Self {
            content,
            cursor_pos: self.cursor_pos + 1,
            mode: self.mode,
        }
    }

    pub fn delete_char(&self) -> Self {
        if self.cursor_pos == 0 {
            return self.clone();
        }
        let mut content = self.content.clone();
        let new_cursor = self.cursor_pos - 1;
        let byte_pos = self.byte_position_at(new_cursor);
        let next_byte = self.byte_position();
        content.replace_range(byte_pos..next_byte, "");
        Self {
            content,
            cursor_pos: new_cursor,
            mode: self.mode,
        }
    }

    pub fn take_content(&self) -> (Self, String) {
        let content = self.content.clone();
        let cleared = Self {
            content: String::new(),
            cursor_pos: 0,
            mode: self.mode,
        };
        (cleared, content)
    }

    pub fn set_mode(&self, mode: Mode) -> Self {
        Self {
            content: self.content.clone(),
            cursor_pos: self.cursor_pos,
            mode,
        }
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
                *self = self.insert_char(*c);
                None
            }
            Action::DeleteChar => {
                *self = self.delete_char();
                None
            }
            Action::SwitchMode(mode) => {
                *self = self.set_mode(*mode);
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let mode_label = self.mode.label();

        let paragraph = Paragraph::new(self.content.as_str()).block(
            Block::default()
                .title(format!(" Input [{mode_label}] "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

        frame.render_widget(paragraph, area);

        // Show cursor when in insert mode
        if focused && self.mode == Mode::Insert {
            let x = area.x + 1 + self.cursor_pos as u16;
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
    fn insert_char_returns_new_state() {
        let input = InputBox::new();
        let updated = input.insert_char('h');
        assert_eq!(updated.content, "h");
        assert_eq!(updated.cursor_pos, 1);
        // Original unchanged
        assert!(input.content.is_empty());
    }

    #[test]
    fn insert_multiple_chars() {
        let input = InputBox::new()
            .insert_char('h')
            .insert_char('i');
        assert_eq!(input.content, "hi");
        assert_eq!(input.cursor_pos, 2);
    }

    #[test]
    fn delete_char_removes_last() {
        let input = InputBox::new()
            .insert_char('a')
            .insert_char('b')
            .delete_char();
        assert_eq!(input.content, "a");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn delete_char_at_start_does_nothing() {
        let input = InputBox::new().delete_char();
        assert!(input.content.is_empty());
        assert_eq!(input.cursor_pos, 0);
    }

    #[test]
    fn take_content_clears_and_returns() {
        let input = InputBox::new()
            .insert_char('h')
            .insert_char('i');
        let (cleared, content) = input.take_content();
        assert_eq!(content, "hi");
        assert!(cleared.content.is_empty());
        assert_eq!(cleared.cursor_pos, 0);
    }

    #[test]
    fn set_mode_returns_new_state() {
        let input = InputBox::new();
        let insert = input.set_mode(Mode::Insert);
        assert_eq!(insert.mode, Mode::Insert);
        assert_eq!(input.mode, Mode::Normal); // original unchanged
    }

    #[test]
    fn handles_unicode_correctly() {
        let input = InputBox::new()
            .insert_char('🦀')
            .insert_char('!')
            .delete_char();
        assert_eq!(input.content, "🦀");
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
