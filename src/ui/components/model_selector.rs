use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::event::types::Action;
use crate::ui::theme::Theme;

use super::Component;

/// Top bar showing the current model and provider.
#[derive(Debug, Clone)]
pub struct ModelSelector {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) mcp_server_count: usize,
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

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        let line = Line::from(vec![
            Span::styled(" Model: ", Style::default().fg(theme.label)),
            Span::styled(
                &self.model,
                Style::default()
                    .fg(theme.highlight)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Provider: ", Style::default().fg(theme.label)),
            Span::styled(
                &self.provider,
                Style::default().fg(theme.provider_name),
            ),
            Span::styled("  MCP: ", Style::default().fg(theme.label)),
            Span::styled(
                format!("{} servers", self.mcp_server_count),
                Style::default().fg(theme.mcp_count),
            ),
        ]);

        let paragraph = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border_unfocused)),
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
