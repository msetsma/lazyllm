pub mod components;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::App;

/// Renders the entire application UI.
pub fn render(app: &App, frame: &mut Frame) {
    let size = frame.area();
    let theme = &app.theme;

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

    // Horizontal body: chat list | chat view
    let sidebar_width = if app.config.ui.show_sidebar {
        app.config.ui.sidebar_width
    } else {
        0
    };
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(sidebar_width),
            Constraint::Min(20),
        ])
        .split(body_area);

    let chat_list_area = body_layout[0];
    let chat_view_area = body_layout[1];

    // Render each component
    use crate::event::types::FocusTarget;
    use components::Component;

    app.model_selector.render(frame, model_area, false, theme);
    if app.config.ui.show_sidebar {
        app.chat_list.render(frame, chat_list_area, app.focus == FocusTarget::ChatList, theme);
    }
    app.chat_view.render(frame, chat_view_area, app.focus == FocusTarget::ChatView, theme);
    app.input_box.render(frame, input_area, app.focus == FocusTarget::Input, theme);
    app.status_bar.render(frame, status_area, false, theme);

    // Render overlays last (on top)
    app.tool_panel.render(frame, size, false, theme);
    app.help_overlay.render(frame, size, false, theme);
    app.model_popup.render(frame, size, false, theme);
}

/// Convert a ratatui Buffer to a human-readable string for snapshot testing.
#[cfg(test)]
fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let area = buf.area;
    let mut output = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &buf[(x, y)];
            output.push_str(cell.symbol());
        }
        // Trim trailing whitespace per line for cleaner snapshots
        let trimmed = output.trim_end();
        output.truncate(trimmed.len());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use crate::config::types::AppConfig;
    use crate::event::types::{Action, FocusTarget, Mode};
    use crate::llm::ProviderRegistry;
    use crate::ui::components::Component;
    use crate::ui::components::chat_view::{ChatMessage, MessageRole};
    use crate::ui::theme::Theme;

    fn test_app() -> crate::app::App {
        crate::app::App::new(AppConfig::default(), ProviderRegistry::new())
    }

    fn test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(width, height)).unwrap()
    }

    // ── Component render tests (TestBackend) ──────────────────────────

    #[test]
    fn status_bar_renders_mode_label() {
        let mut terminal = test_terminal(60, 1);
        let theme = Theme::default();
        let bar = crate::ui::components::status_bar::StatusBar::new();

        terminal
            .draw(|frame| {
                bar.render(frame, frame.area(), false, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains("NORMAL"), "Expected NORMAL label in: {output}");
        assert!(output.contains("quit"), "Expected hint text in: {output}");
    }

    #[test]
    fn status_bar_renders_insert_mode() {
        let mut terminal = test_terminal(60, 1);
        let theme = Theme::default();
        let mut bar = crate::ui::components::status_bar::StatusBar::new();
        bar.set_mode(Mode::Insert);

        terminal
            .draw(|frame| {
                bar.render(frame, frame.area(), false, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains("INSERT"), "Expected INSERT label in: {output}");
        assert!(output.contains("send"), "Expected insert hints in: {output}");
    }

    #[test]
    fn status_bar_renders_status_message() {
        let mut terminal = test_terminal(100, 1);
        let theme = Theme::default();
        let mut bar = crate::ui::components::status_bar::StatusBar::new();
        bar.set_status("streaming...".to_string());

        terminal
            .draw(|frame| {
                bar.render(frame, frame.area(), false, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("streaming..."),
            "Expected status message in: {output}"
        );
    }

    #[test]
    fn input_box_renders_content() {
        let mut terminal = test_terminal(40, 3);
        let theme = Theme::default();
        let mut input = crate::ui::components::input_box::InputBox::new();
        input.insert_char('H');
        input.insert_char('i');

        terminal
            .draw(|frame| {
                input.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains("Hi"), "Expected input content in: {output}");
        assert!(
            output.contains("Input"),
            "Expected Input title in: {output}"
        );
    }

    #[test]
    fn input_box_renders_command_prefix() {
        let mut terminal = test_terminal(40, 3);
        let theme = Theme::default();
        let mut input = crate::ui::components::input_box::InputBox::new();
        input.set_mode(Mode::Command);
        input.insert_char('q');

        terminal
            .draw(|frame| {
                input.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains(":q"), "Expected :q command prefix in: {output}");
    }

    #[test]
    fn input_box_renders_search_prefix() {
        let mut terminal = test_terminal(40, 3);
        let theme = Theme::default();
        let mut input = crate::ui::components::input_box::InputBox::new();
        input.set_mode(Mode::Search);
        input.insert_char('f');
        input.insert_char('o');
        input.insert_char('o');

        terminal
            .draw(|frame| {
                input.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("/foo"),
            "Expected /foo search prefix in: {output}"
        );
    }

    #[test]
    fn chat_view_renders_empty_state() {
        let mut terminal = test_terminal(40, 10);
        let theme = Theme::default();
        let view = crate::ui::components::chat_view::ChatView::new();

        terminal
            .draw(|frame| {
                view.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains("Chat"), "Expected Chat border title in: {output}");
    }

    #[test]
    fn chat_view_renders_messages() {
        let mut terminal = test_terminal(60, 20);
        let theme = Theme::default();
        let mut view = crate::ui::components::chat_view::ChatView::new();
        view.set_markdown_rendering(false);
        view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "Hello there".to_string(),
            timestamp: None,
        });
        view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "Hi! How can I help?".to_string(),
            timestamp: None,
        });

        terminal
            .draw(|frame| {
                view.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("Hello there"),
            "Expected user message in: {output}"
        );
        assert!(
            output.contains("Hi! How can I help?"),
            "Expected assistant message in: {output}"
        );
    }

    #[test]
    fn chat_list_renders_items() {
        let mut terminal = test_terminal(25, 10);
        let theme = Theme::default();
        let list = crate::ui::components::chat_list::ChatList::from_items(vec![
            "First Chat".to_string(),
            "Second Chat".to_string(),
        ]);

        terminal
            .draw(|frame| {
                list.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(output.contains("Chats"), "Expected Chats title in: {output}");
        assert!(
            output.contains("First Chat"),
            "Expected first item in: {output}"
        );
        assert!(
            output.contains("Second Chat"),
            "Expected second item in: {output}"
        );
    }

    #[test]
    fn chat_list_shows_highlight_symbol() {
        let mut terminal = test_terminal(25, 10);
        let theme = Theme::default();
        let list = crate::ui::components::chat_list::ChatList::from_items(vec![
            "Selected".to_string(),
            "Other".to_string(),
        ]);

        terminal
            .draw(|frame| {
                list.render(frame, frame.area(), true, &theme);
            })
            .unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("> Selected"),
            "Expected highlight symbol on selected item in: {output}"
        );
    }

    // ── Full app render tests ─────────────────────────────────────────

    #[test]
    fn full_app_renders_without_panic() {
        let app = test_app();
        let mut terminal = test_terminal(80, 24);

        terminal.draw(|frame| render(&app, frame)).unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        // Should contain the status bar with NORMAL mode
        assert!(
            output.contains("NORMAL"),
            "Expected NORMAL in full render: {output}"
        );
    }

    #[test]
    fn full_app_render_with_messages() {
        let mut app = test_app();
        app.config.ui.show_sidebar = false;
        app.chat_view.set_markdown_rendering(false);
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "What is Rust?".to_string(),
            timestamp: None,
        });
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "Rust is a systems programming language.".to_string(),
            timestamp: None,
        });

        let mut terminal = test_terminal(80, 24);
        terminal.draw(|frame| render(&app, frame)).unwrap();

        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("What is Rust?"),
            "Expected user message in full render: {output}"
        );
        assert!(
            output.contains("Rust is a systems programming language"),
            "Expected assistant message in full render: {output}"
        );
    }

    #[test]
    fn full_app_focus_changes_border_style() {
        let mut app = test_app();
        // Default focus is on ChatView — switch to Input
        app.focus = FocusTarget::Input;

        let mut terminal = test_terminal(80, 24);
        terminal.draw(|frame| render(&app, frame)).unwrap();

        // We can't easily check color in the string, but we can verify it renders
        let output = buffer_to_string(terminal.backend().buffer());
        assert!(
            output.contains("Input"),
            "Expected Input box in render: {output}"
        );
    }

    // ── Snapshot tests (insta) ────────────────────────────────────────

    #[test]
    fn snapshot_empty_app() {
        let app = test_app();
        let mut terminal = test_terminal(80, 24);
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let output = buffer_to_string(terminal.backend().buffer());
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_app_with_conversation() {
        let mut app = test_app();
        app.chat_view.set_markdown_rendering(false);
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::User,
            content: "Hello!".to_string(),
            timestamp: None,
        });
        app.chat_view.add_message(ChatMessage {
            role: MessageRole::Assistant,
            content: "Hi there! How can I help you today?".to_string(),
            timestamp: None,
        });

        let mut terminal = test_terminal(80, 24);
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let output = buffer_to_string(terminal.backend().buffer());
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_insert_mode() {
        let mut app = test_app();
        app.mode = Mode::Insert;
        app.focus = FocusTarget::Input;
        app.input_box.set_mode(Mode::Insert);
        app.status_bar.set_mode(Mode::Insert);
        app.input_box.insert_char('H');
        app.input_box.insert_char('e');
        app.input_box.insert_char('l');
        app.input_box.insert_char('l');
        app.input_box.insert_char('o');

        let mut terminal = test_terminal(80, 24);
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let output = buffer_to_string(terminal.backend().buffer());
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_status_bar_all_modes() {
        let theme = Theme::default();
        let modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual,
            Mode::Command,
            Mode::Search,
        ];

        let mut combined = String::new();
        for mode in &modes {
            let mut bar = crate::ui::components::status_bar::StatusBar::new();
            bar.set_mode(*mode);

            let mut terminal = test_terminal(80, 1);
            terminal
                .draw(|frame| bar.render(frame, frame.area(), false, &theme))
                .unwrap();
            combined.push_str(&buffer_to_string(terminal.backend().buffer()));
        }
        insta::assert_snapshot!(combined);
    }
}
