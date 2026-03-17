use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::event::types::Action;

use super::Component;

/// Top bar showing the current model and provider.
#[derive(Debug, Clone)]
pub struct ModelSelector {
    pub provider: String,
    pub model: String,
    pub mcp_server_count: usize,
}

impl ModelSelector {
    pub fn new(provider: String, model: String) -> Self {
        Self {
            provider,
            model,
            mcp_server_count: 0,
        }
    }

    pub fn with_mcp_count(&self, count: usize) -> Self {
        Self {
            provider: self.provider.clone(),
            model: self.model.clone(),
            mcp_server_count: count,
        }
    }
}

impl Component for ModelSelector {
    fn handle_action(&mut self, _action: &Action) -> Option<Action> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool) {
        let line = Line::from(vec![
            Span::styled(" Model: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &self.model,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &self.provider,
                Style::default().fg(Color::Green),
            ),
            Span::styled("  MCP: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} servers", self.mcp_server_count),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        let paragraph = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_model_selector() {
        let sel = ModelSelector::new("openai".to_string(), "gpt-4o".to_string());
        assert_eq!(sel.provider, "openai");
        assert_eq!(sel.model, "gpt-4o");
        assert_eq!(sel.mcp_server_count, 0);
    }

    #[test]
    fn with_mcp_count_returns_new_instance() {
        let sel = ModelSelector::new("openai".to_string(), "gpt-4o".to_string());
        let updated = sel.with_mcp_count(3);
        assert_eq!(updated.mcp_server_count, 3);
        // Original unchanged
        assert_eq!(sel.mcp_server_count, 0);
    }
}
