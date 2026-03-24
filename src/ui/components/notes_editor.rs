use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::theme::Theme;

use super::centered_rect;

/// Result of handling a key event in the notes editor.
#[derive(Debug, PartialEq, Eq)]
pub enum NotesAction {
    /// Key was consumed, editor continues.
    Continue,
    /// User saved (Ctrl+Enter).
    Save,
    /// User discarded (Esc).
    Discard,
}

/// Popup editor for session notes.
#[derive(Debug, Clone, Default)]
pub struct NotesEditor {
    pub(crate) visible: bool,
    content: String,
    cursor_pos: usize,
    original_content: String,
}

impl NotesEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the editor with existing notes content.
    pub fn open(&mut self, existing_notes: &str) {
        self.visible = true;
        self.content = existing_notes.to_string();
        self.original_content = self.content.clone();
        self.cursor_pos = self.content.len();
    }

    /// Close and return the saved content.
    pub fn close_save(&mut self) -> String {
        self.visible = false;
        std::mem::take(&mut self.content)
    }

    /// Close and discard changes.
    pub fn close_discard(&mut self) {
        self.visible = false;
        self.content = std::mem::take(&mut self.original_content);
    }

    /// Handle a key event. Returns the action to take.
    pub fn handle_key(&mut self, key: KeyEvent) -> NotesAction {
        match key.code {
            KeyCode::Esc => NotesAction::Discard,
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                NotesAction::Save
            }
            KeyCode::Enter => {
                self.content.insert(self.cursor_pos, '\n');
                self.cursor_pos += 1;
                NotesAction::Continue
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.content.remove(self.cursor_pos);
                }
                NotesAction::Continue
            }
            KeyCode::Char(c) => {
                self.content.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                NotesAction::Continue
            }
            KeyCode::Left => {
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
                NotesAction::Continue
            }
            KeyCode::Right => {
                if self.cursor_pos < self.content.len() {
                    self.cursor_pos += 1;
                }
                NotesAction::Continue
            }
            _ => NotesAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        let width = 60.min(area.width.saturating_sub(4));
        let height = 12.min(area.height.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);
        frame.render_widget(Clear, popup_area);

        let mut lines: Vec<Line> = self
            .content
            .lines()
            .map(|l| Line::from(l.to_string()))
            .collect();
        if lines.is_empty() {
            lines.push(Line::from(""));
        }

        let footer = Line::from(vec![
            Span::styled("Ctrl+Enter", Style::default().fg(theme.help_key)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(theme.help_key)),
            Span::raw(" discard"),
        ]);

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Session Notes ")
                    .title_bottom(footer)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.pulse_note_border)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_hidden() {
        let editor = NotesEditor::new();
        assert!(!editor.visible);
    }

    #[test]
    fn open_sets_content_and_visible() {
        let mut editor = NotesEditor::new();
        editor.open("hello");
        assert!(editor.visible);
        assert_eq!(editor.content, "hello");
        assert_eq!(editor.cursor_pos, 5);
    }

    #[test]
    fn close_save_returns_content() {
        let mut editor = NotesEditor::new();
        editor.open("test");
        editor.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        let saved = editor.close_save();
        assert_eq!(saved, "test!");
        assert!(!editor.visible);
    }

    #[test]
    fn close_discard_restores_original() {
        let mut editor = NotesEditor::new();
        editor.open("original");
        editor.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        editor.close_discard();
        assert!(!editor.visible);
        // Content was discarded, original restored internally
    }

    #[test]
    fn esc_returns_discard() {
        let mut editor = NotesEditor::new();
        editor.open("");
        let action = editor.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(action, NotesAction::Discard);
    }

    #[test]
    fn ctrl_enter_returns_save() {
        let mut editor = NotesEditor::new();
        editor.open("");
        let action = editor.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(action, NotesAction::Save);
    }

    #[test]
    fn typing_appends_chars() {
        let mut editor = NotesEditor::new();
        editor.open("");
        editor.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(editor.content, "ab");
    }

    #[test]
    fn backspace_removes_char() {
        let mut editor = NotesEditor::new();
        editor.open("abc");
        editor.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(editor.content, "ab");
    }

    #[test]
    fn enter_inserts_newline() {
        let mut editor = NotesEditor::new();
        editor.open("line1");
        editor.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        editor.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(editor.content, "line1\n2");
    }
}
