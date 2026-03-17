use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::event::types::Action;

use super::Component;

/// Modal overlay showing keybinding help.
#[derive(Debug, Clone, Default)]
pub struct HelpOverlay {
    pub visible: bool,
}

impl HelpOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle(&self) -> Self {
        Self {
            visible: !self.visible,
        }
    }

    fn help_lines(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                "Keybindings",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            section_header("Navigation"),
            binding("Tab / Shift+Tab", "Switch panel focus"),
            binding("h / l", "Focus left / right panel"),
            binding("j / k", "Scroll down / up"),
            Line::from(""),
            section_header("Modes"),
            binding("i", "Enter Insert mode"),
            binding("v", "Enter Visual mode"),
            binding(":", "Enter Command mode"),
            binding("Esc", "Return to Normal mode"),
            Line::from(""),
            section_header("Actions"),
            binding("Enter (Insert)", "Send message"),
            binding("n", "New chat"),
            binding("d", "Delete chat"),
            binding("?", "Toggle this help"),
            binding("q", "Quit"),
            binding("Ctrl+C", "Force quit"),
        ]
    }
}

fn section_header(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))
}

fn binding(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<20}"),
            Style::default().fg(Color::Green),
        ),
        Span::raw(desc.to_string()),
    ])
}

/// Center a rect within a parent area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0]);
    horizontal[0]
}

impl Component for HelpOverlay {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ToggleHelp => {
                *self = self.toggle();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool) {
        if !self.visible {
            return;
        }

        let popup_area = centered_rect(50, 22, area);
        frame.render_widget(Clear, popup_area);

        let paragraph = Paragraph::new(self.help_lines())
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
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
        let help = HelpOverlay::new();
        let shown = help.toggle();
        assert!(shown.visible);
        let hidden = shown.toggle();
        assert!(!hidden.visible);
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
        let lines = help.help_lines();
        assert!(!lines.is_empty());
    }

    #[test]
    fn centered_rect_fits_within_area() {
        let area = Rect::new(0, 0, 100, 50);
        let centered = centered_rect(40, 20, area);
        assert!(centered.x + centered.width <= area.width);
        assert!(centered.y + centered.height <= area.height);
    }
}
