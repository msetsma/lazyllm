pub mod components;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::App;

/// Renders the entire application UI.
pub fn render(app: &App, frame: &mut Frame) {
    let size = frame.area();

    // Vertical: model bar (2) | body (fill) | input (3) | status (1)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // model selector
            Constraint::Min(5),    // body
            Constraint::Length(3), // input
            Constraint::Length(1), // status bar
        ])
        .split(size);

    let model_area = vertical[0];
    let body_area = vertical[1];
    let input_area = vertical[2];
    let status_area = vertical[3];

    // Horizontal body: chat list | chat view | tool panel
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(app.config.ui.sidebar_width),
            Constraint::Min(20),
            Constraint::Length(app.config.ui.tool_panel_width),
        ])
        .split(body_area);

    let chat_list_area = body_layout[0];
    let chat_view_area = body_layout[1];
    let tool_panel_area = body_layout[2];

    // Render each component
    use crate::event::types::FocusTarget;
    use components::Component;

    app.model_selector.render(frame, model_area, false);
    app.chat_list.render(frame, chat_list_area, app.focus == FocusTarget::ChatList);
    app.chat_view.render(frame, chat_view_area, app.focus == FocusTarget::ChatView);
    app.tool_panel.render(frame, tool_panel_area, app.focus == FocusTarget::ToolPanel);
    app.input_box.render(frame, input_area, app.focus == FocusTarget::Input);
    app.status_bar.render(frame, status_area, false);

    // Render overlays last (on top)
    app.help_overlay.render(frame, size, false);
}
