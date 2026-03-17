pub mod chat_list;
pub mod chat_view;
pub mod help_overlay;
pub mod input_box;
pub mod model_selector;
pub mod status_bar;
pub mod tool_panel;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::event::types::Action;

/// Trait for all UI components (panels, overlays).
pub trait Component {
    /// Process an action and return an optional follow-up action.
    fn handle_action(&mut self, action: &Action) -> Option<Action>;

    /// Render the component into the given area.
    fn render(&self, frame: &mut Frame, area: Rect, focused: bool);
}
