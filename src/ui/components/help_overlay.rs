use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::event::types::Action;
use crate::ui::theme::Theme;

use super::{Component, centered_rect};

/// Modal overlay showing keybinding help.
#[derive(Debug, Clone, Default)]
pub struct HelpOverlay {
    pub(crate) visible: bool,
}

impl HelpOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    fn help_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                "Keybindings",
                Style::default()
                    .fg(theme.help_title)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            section_header("Navigation", theme),
            binding("Tab / Shift+Tab", "Switch panel focus", theme),
            binding("h / l", "Focus left / right panel", theme),
            binding("j / k", "Scroll down / up", theme),
            Line::from(""),
            section_header("Modes", theme),
            binding("i", "Enter Insert mode", theme),
            binding("v", "Enter Visual mode", theme),
            binding(":", "Enter Command mode", theme),
            binding("Esc", "Return to Normal mode", theme),
            Line::from(""),
            section_header("Actions", theme),
            binding("Enter (Insert)", "Send message", theme),
            binding("y (Visual)", "Copy last response", theme),
            binding("n", "New chat", theme),
            binding("d", "Delete chat", theme),
            binding("m", "Select model", theme),
            binding("/", "Search in conversation", theme),
            binding("?", "Toggle this help", theme),
            binding("q", "Quit", theme),
            binding("Ctrl+C", "Force quit", theme),
            Line::from(""),
            section_header("Search (/ mode)", theme),
            binding("Enter / Down", "Next match", theme),
            binding("Up", "Previous match", theme),
            binding("Esc", "Exit search", theme),
            Line::from(""),
            section_header("Commands (: mode)", theme),
            binding(":quit / :q", "Quit", theme),
            binding(":model <id>", "Switch model", theme),
            binding(":provider <name>", "Switch provider", theme),
            binding(":new", "New chat", theme),
            binding(":delete / :del", "Delete chat", theme),
            binding(":clear", "Clear messages", theme),
            binding(":context <name>", "Set active context", theme),
            binding(":ctx none", "Clear context", theme),
            binding(":help", "Toggle help", theme),
        ]
    }
}

fn section_header(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme.help_section)
            .add_modifier(Modifier::BOLD),
    ))
}

fn binding(key: &str, desc: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<20}"),
            Style::default().fg(theme.help_key),
        ),
        Span::raw(desc.to_string()),
    ])
}

impl Component for HelpOverlay {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ToggleHelp => {
                self.toggle();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        if !self.visible {
            return;
        }

        let lines = self.help_lines(theme);
        let content_height = lines.len() as u16 + 2; // +2 for borders
        let height = content_height.min(area.height.saturating_sub(4));
        let popup_area = centered_rect(60, height, area);
        frame.render_widget(Clear, popup_area);

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.popup_border)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_overlay_starts_hidden() {
        let help = HelpOverlay::new();
        assert!(!help.visible);
    }

    #[test]
    fn toggle_flips_visibility() {
        let mut help = HelpOverlay::new();
        help.toggle();
        assert!(help.visible);
        help.toggle();
        assert!(!help.visible);
    }

    #[test]
    fn handle_action_toggle_help() {
        let mut help = HelpOverlay::new();
        help.handle_action(&Action::ToggleHelp);
        assert!(help.visible);
        help.handle_action(&Action::ToggleHelp);
        assert!(!help.visible);
    }

    #[test]
    fn help_lines_are_not_empty() {
        let help = HelpOverlay::new();
        let theme = Theme::default();
        let lines = help.help_lines(&theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn help_lines_include_commands() {
        let help = HelpOverlay::new();
        let theme = Theme::default();
        let lines = help.help_lines(&theme);
        let content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains(":quit"));
        assert!(content.contains(":model"));
    }

    #[test]
    fn centered_rect_fits_within_area() {
        let area = Rect::new(0, 0, 100, 50);
        let centered = centered_rect(40, 20, area);
        assert!(centered.x + centered.width <= area.width);
        assert!(centered.y + centered.height <= area.height);
    }
}
