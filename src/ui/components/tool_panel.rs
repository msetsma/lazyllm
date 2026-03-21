use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::event::types::Action;
use crate::ui::theme::Theme;

use super::{Component, centered_rect};

/// Popup overlay showing available MCP tools.
#[derive(Debug, Clone, Default)]
pub struct ToolPanel {
    pub(crate) servers: Vec<ServerTools>,
    pub(crate) visible: bool,
}

#[derive(Debug, Clone)]
pub struct ServerTools {
    pub server_name: String,
    pub tools: Vec<String>,
}

impl ToolPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }
}

impl Component for ToolPanel {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ToggleToolPanel => {
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

        let mut items: Vec<ListItem> = Vec::new();

        for server in &self.servers {
            items.push(ListItem::new(Line::styled(
                &server.server_name,
                Style::default().fg(theme.server_name),
            )));
            for tool in &server.tools {
                items.push(ListItem::new(Line::from(format!("  {tool}"))));
            }
        }

        if items.is_empty() {
            items.push(ListItem::new(Line::styled(
                "No MCP servers",
                Style::default().fg(theme.empty_state),
            )));
        }

        let content_height = (items.len() as u16 + 2).min(area.height.saturating_sub(4));
        let popup_area = centered_rect(50, content_height, area);
        frame.render_widget(Clear, popup_area);

        let list = List::new(items).block(
            Block::default()
                .title(" Tools ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.popup_border)),
        );

        frame.render_widget(list, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures a new tool panel starts with no servers listed.
    #[test]
    fn new_tool_panel_is_empty() {
        let panel = ToolPanel::new();
        assert!(panel.servers.is_empty());
    }

    /// Ensures the tool panel is hidden by default.
    #[test]
    fn tool_panel_starts_hidden() {
        let panel = ToolPanel::new();
        assert!(!panel.visible);
    }

    /// Verifies toggle flips the panel between visible and hidden.
    #[test]
    fn toggle_flips_visibility() {
        let mut panel = ToolPanel::new();
        panel.toggle();
        assert!(panel.visible);
        panel.toggle();
        assert!(!panel.visible);
    }

    /// Verifies close sets visibility to false.
    #[test]
    fn close_hides_panel() {
        let mut panel = ToolPanel::new();
        panel.visible = true;
        panel.close();
        assert!(!panel.visible);
    }

    /// Ensures the ToggleToolPanel action toggles visibility via handle_action.
    #[test]
    fn handle_action_toggle_tool_panel() {
        let mut panel = ToolPanel::new();
        panel.handle_action(&Action::ToggleToolPanel);
        assert!(panel.visible);
        panel.handle_action(&Action::ToggleToolPanel);
        assert!(!panel.visible);
    }
}
