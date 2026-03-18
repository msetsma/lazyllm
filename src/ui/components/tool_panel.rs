use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::event::types::Action;
use crate::ui::theme::Theme;

use super::Component;

/// Right panel showing available MCP tools.
#[derive(Debug, Clone, Default)]
pub struct ToolPanel {
    pub(crate) servers: Vec<ServerTools>,
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
}

impl Component for ToolPanel {
    fn handle_action(&mut self, _action: &Action) -> Option<Action> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = super::focused_border_style(focused, theme);

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

        let list = List::new(items).block(
            Block::default()
                .title(" Tools ")
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        frame.render_widget(list, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tool_panel_is_empty() {
        let panel = ToolPanel::new();
        assert!(panel.servers.is_empty());
    }

    #[test]
    fn handle_action_returns_none() {
        let mut panel = ToolPanel::new();
        assert_eq!(panel.handle_action(&Action::ScrollDown), None);
    }
}
