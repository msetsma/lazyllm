pub mod chat_list;
pub mod chat_view;
pub mod help_overlay;
pub mod input_box;
pub mod model_popup;
pub mod model_selector;
pub mod status_bar;
pub mod tool_panel;

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};

use crate::event::types::Action;
use crate::ui::theme::Theme;

/// Trait for all UI components (panels, overlays).
///
/// Components own their state and use `&mut self` methods for all mutations.
/// `handle_action` delegates to these methods. Avoid builder-style methods
/// that clone and return `Self` — mutate in place instead.
pub trait Component {
    /// Process an action and return an optional follow-up action.
    fn handle_action(&mut self, action: &Action) -> Option<Action>;

    /// Render the component into the given area.
    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme);
}

/// Returns the border style for a component based on focus state.
pub fn focused_border_style(focused: bool, theme: &Theme) -> ratatui::style::Style {
    use ratatui::style::Style;
    if focused {
        Style::default().fg(theme.border_focused)
    } else {
        Style::default().fg(theme.border_unfocused)
    }
}

/// Center a rect within a parent area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0]);
    horizontal[0]
}
