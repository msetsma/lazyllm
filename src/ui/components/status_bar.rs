use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::event::types::{Action, Mode};

use super::Component;

/// Bottom status bar showing mode, keybinding hints, and status.
#[derive(Debug, Clone)]
pub struct StatusBar {
    pub mode: Mode,
    pub status_message: Option<String>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            mode: Mode::Normal,
            status_message: None,
        }
    }

    pub fn with_mode(&self, mode: Mode) -> Self {
        Self {
            mode,
            status_message: self.status_message.clone(),
        }
    }

    pub fn with_status(&self, message: String) -> Self {
        Self {
            mode: self.mode,
            status_message: Some(message),
        }
    }

    fn hint_text(&self) -> &str {
        match self.mode {
            Mode::Normal => "q:quit  i:insert  Tab:focus  j/k:scroll  ?:help",
            Mode::Insert => "Esc:normal  Enter:send  type to compose",
            Mode::Visual => "Esc:normal  j/k:scroll  y:copy",
            Mode::Command => "Esc:cancel  Enter:execute",
        }
    }
}

impl Component for StatusBar {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SwitchMode(mode) => {
                *self = self.with_mode(*mode);
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool) {
        let mode_color = match self.mode {
            Mode::Normal => Color::Blue,
            Mode::Insert => Color::Green,
            Mode::Visual => Color::Magenta,
            Mode::Command => Color::Yellow,
        };

        let mut spans = vec![
            Span::styled(
                format!(" {} ", self.mode.label()),
                Style::default().fg(Color::Black).bg(mode_color),
            ),
            Span::styled(
                format!("  {}", self.hint_text()),
                Style::default().fg(Color::DarkGray),
            ),
        ];

        if let Some(ref status) = self.status_message {
            spans.push(Span::styled(
                format!("  {status}"),
                Style::default().fg(Color::Yellow),
            ));
        }

        let paragraph = Paragraph::new(Line::from(spans));
        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_status_bar_defaults_to_normal() {
        let bar = StatusBar::new();
        assert_eq!(bar.mode, Mode::Normal);
        assert!(bar.status_message.is_none());
    }

    #[test]
    fn with_mode_returns_new_instance() {
        let bar = StatusBar::new();
        let updated = bar.with_mode(Mode::Insert);
        assert_eq!(updated.mode, Mode::Insert);
        assert_eq!(bar.mode, Mode::Normal); // unchanged
    }

    #[test]
    fn with_status_returns_new_instance() {
        let bar = StatusBar::new();
        let updated = bar.with_status("streaming...".to_string());
        assert_eq!(updated.status_message.as_deref(), Some("streaming..."));
        assert!(bar.status_message.is_none()); // unchanged
    }

    #[test]
    fn hint_text_varies_by_mode() {
        let bar = StatusBar::new();
        assert!(bar.hint_text().contains("quit"));

        let insert = bar.with_mode(Mode::Insert);
        assert!(insert.hint_text().contains("send"));

        let visual = bar.with_mode(Mode::Visual);
        assert!(visual.hint_text().contains("copy"));

        let command = bar.with_mode(Mode::Command);
        assert!(command.hint_text().contains("execute"));
    }

    #[test]
    fn handle_action_updates_mode() {
        let mut bar = StatusBar::new();
        bar.handle_action(&Action::SwitchMode(Mode::Insert));
        assert_eq!(bar.mode, Mode::Insert);
    }
}
