use crossterm::event::KeyEvent;

/// Actions that can be dispatched to update application state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    SwitchMode(Mode),
    FocusNext,
    FocusPrev,
    ScrollUp,
    ScrollDown,
    SelectItem,
    NewChat,
    DeleteChat,
    SendMessage,
    InsertChar(char),
    DeleteChar,
    InputSubmit,
    ToggleHelp,
    ToggleModelSelector,
    SelectModel,
    Resize(u16, u16),
    Tick,
    None,
}

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
}

impl Mode {
    pub fn label(&self) -> &str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Command => "COMMAND",
        }
    }
}

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    ChatList,
    #[default]
    ChatView,
    ToolPanel,
    Input,
}

impl FocusTarget {
    pub fn next(self) -> Self {
        match self {
            FocusTarget::ChatList => FocusTarget::ChatView,
            FocusTarget::ChatView => FocusTarget::ToolPanel,
            FocusTarget::ToolPanel => FocusTarget::Input,
            FocusTarget::Input => FocusTarget::ChatList,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            FocusTarget::ChatList => FocusTarget::Input,
            FocusTarget::ChatView => FocusTarget::ChatList,
            FocusTarget::ToolPanel => FocusTarget::ChatView,
            FocusTarget::Input => FocusTarget::ToolPanel,
        }
    }
}

/// Raw terminal events before they are mapped to actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_labels_are_correct() {
        assert_eq!(Mode::Normal.label(), "NORMAL");
        assert_eq!(Mode::Insert.label(), "INSERT");
        assert_eq!(Mode::Visual.label(), "VISUAL");
        assert_eq!(Mode::Command.label(), "COMMAND");
    }

    #[test]
    fn focus_next_cycles_through_all_panels() {
        let start = FocusTarget::ChatList;
        let second = start.next();
        assert_eq!(second, FocusTarget::ChatView);
        let third = second.next();
        assert_eq!(third, FocusTarget::ToolPanel);
        let fourth = third.next();
        assert_eq!(fourth, FocusTarget::Input);
        let back = fourth.next();
        assert_eq!(back, FocusTarget::ChatList);
    }

    #[test]
    fn focus_prev_cycles_backwards() {
        let start = FocusTarget::ChatList;
        let prev = start.prev();
        assert_eq!(prev, FocusTarget::Input);
        let prev2 = prev.prev();
        assert_eq!(prev2, FocusTarget::ToolPanel);
    }

    #[test]
    fn focus_next_and_prev_are_inverse() {
        for target in [
            FocusTarget::ChatList,
            FocusTarget::ChatView,
            FocusTarget::ToolPanel,
            FocusTarget::Input,
        ] {
            assert_eq!(target.next().prev(), target);
            assert_eq!(target.prev().next(), target);
        }
    }

    #[test]
    fn default_mode_is_normal() {
        assert_eq!(Mode::default(), Mode::Normal);
    }

    #[test]
    fn default_focus_is_chat_view() {
        assert_eq!(FocusTarget::default(), FocusTarget::ChatView);
    }
}
