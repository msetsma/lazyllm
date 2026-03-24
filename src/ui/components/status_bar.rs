use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::event::types::{Action, Mode};
use crate::llm::health::HealthState;
use crate::ui::theme::Theme;

use super::Component;

/// Bottom status bar showing mode, keybinding hints, and status.
#[derive(Debug, Clone)]
pub struct StatusBar {
    pub(crate) mode: Mode,
    pub(crate) status_message: Option<String>,
    pub(crate) health_state: Option<HealthState>,
    pub(crate) usage_pct: Option<u32>,
    pub(crate) has_compaction_summary: bool,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            mode: Mode::Normal,
            status_message: None,
            health_state: None,
            usage_pct: None,
            has_compaction_summary: false,
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }

    fn hint_text(&self) -> &str {
        match self.mode {
            Mode::Normal => "q:quit  i:insert  /:search  Tab:focus  j/k:scroll  ?:help",
            Mode::Insert => "Esc:normal  Enter:send  type to compose",
            Mode::Visual => "Esc:normal  j/k:scroll  y:copy",
            Mode::Command => "Esc:cancel  Enter:execute",
            Mode::Search => "Esc:exit  Enter:next  Up/Down:prev/next  type to search",
        }
    }
}

impl Component for StatusBar {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SwitchMode(mode) => {
                self.set_mode(*mode);
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        let (bg, fg) = match self.mode {
            Mode::Normal => (theme.mode_normal_bg, theme.mode_normal_fg),
            Mode::Insert => (theme.mode_insert_bg, theme.mode_insert_fg),
            Mode::Visual => (theme.mode_visual_bg, theme.mode_visual_fg),
            Mode::Command => (theme.mode_command_bg, theme.mode_command_fg),
            Mode::Search => (theme.highlight, theme.mode_normal_fg),
        };

        let mut spans = vec![
            Span::styled(
                format!(" {} ", self.mode.label()),
                Style::default().fg(fg).bg(bg),
            ),
            Span::styled(
                format!("  {}", self.hint_text()),
                Style::default().fg(theme.hint_text),
            ),
        ];

        if let Some(ref status) = self.status_message {
            spans.push(Span::styled(
                format!("  {status}"),
                Style::default().fg(theme.status_message),
            ));
        }

        if let (Some(health), Some(pct)) = (&self.health_state, self.usage_pct) {
            let color = theme.health_color(health);
            let compaction_indicator = if self.has_compaction_summary { " \u{27F3}" } else { "" };
            spans.push(Span::styled(
                format!("  \u{25C9} {}% {}{}", pct, health.label(), compaction_indicator),
                Style::default().fg(color),
            ));
        }

        let paragraph = Paragraph::new(Line::from(spans));
        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures a new status bar starts in Normal mode with no status message.
    #[test]
    fn new_status_bar_defaults_to_normal() {
        let bar = StatusBar::new();
        assert_eq!(bar.mode, Mode::Normal);
        assert!(bar.status_message.is_none());
    }

    /// Verifies set_mode changes the displayed mode.
    #[test]
    fn set_mode_updates_in_place() {
        let mut bar = StatusBar::new();
        bar.set_mode(Mode::Insert);
        assert_eq!(bar.mode, Mode::Insert);
    }

    /// Verifies set_status stores the message for display.
    #[test]
    fn set_status_updates_in_place() {
        let mut bar = StatusBar::new();
        bar.set_status("streaming...".to_string());
        assert_eq!(bar.status_message.as_deref(), Some("streaming..."));
    }

    /// Ensures hint_text returns mode-appropriate instructions (quit, send, copy, execute).
    #[test]
    fn hint_text_varies_by_mode() {
        let mut bar = StatusBar::new();
        assert!(bar.hint_text().contains("quit"));

        bar.set_mode(Mode::Insert);
        assert!(bar.hint_text().contains("send"));

        bar.set_mode(Mode::Visual);
        assert!(bar.hint_text().contains("copy"));

        bar.set_mode(Mode::Command);
        assert!(bar.hint_text().contains("execute"));
    }

    /// Ensures SwitchMode action updates the status bar's mode display.
    #[test]
    fn handle_action_updates_mode() {
        let mut bar = StatusBar::new();
        bar.handle_action(&Action::SwitchMode(Mode::Insert));
        assert_eq!(bar.mode, Mode::Insert);
    }
}
