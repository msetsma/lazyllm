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
    ToggleToolPanel,
    ToggleModelSelector,
    SelectModel,
    CopySelection,
    SearchNext,
    SearchPrev,
    Resize(u16, u16),
    Tick,
    None,
    /// Toggle the Pulse overlay.
    TogglePulse,
    /// Trigger compaction from within the Pulse overlay.
    PulseCompact,
    /// Trigger compaction with a custom prompt.
    PulseCompactWithPrompt,
    /// Clear old tool results (lightweight compaction).
    PulseClearToolResults,
    /// Undo last compaction by restoring a checkpoint.
    PulseUndoCompaction,
    /// Toggle pin on the selected message.
    PulseTogglePin,
    /// Open the session notes editor.
    PulseEditNotes,
    /// Cycle the compaction mode (auto → client → server → auto).
    PulseSwitchMode,
    /// Raw key event passed through when an overlay needs key interception.
    RawKey(KeyEvent),
}

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
    Search,
}

impl Mode {
    pub fn label(&self) -> &str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
        }
    }
}

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    ChatList,
    #[default]
    ChatView,
    Input,
}

impl FocusTarget {
    pub fn next(self) -> Self {
        match self {
            FocusTarget::ChatList => FocusTarget::ChatView,
            FocusTarget::ChatView => FocusTarget::Input,
            FocusTarget::Input => FocusTarget::ChatList,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            FocusTarget::ChatList => FocusTarget::Input,
            FocusTarget::ChatView => FocusTarget::ChatList,
            FocusTarget::Input => FocusTarget::ChatView,
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

    /// Verifies each Mode variant returns the expected uppercase status-bar label.
    #[test]
    fn mode_labels_are_correct() {
        assert_eq!(Mode::Normal.label(), "NORMAL");
        assert_eq!(Mode::Insert.label(), "INSERT");
        assert_eq!(Mode::Visual.label(), "VISUAL");
        assert_eq!(Mode::Command.label(), "COMMAND");
    }

    /// Ensures focus_next cycles through all three panels and wraps back to the start.
    #[test]
    fn focus_next_cycles_through_all_panels() {
        let start = FocusTarget::ChatList;
        let second = start.next();
        assert_eq!(second, FocusTarget::ChatView);
        let third = second.next();
        assert_eq!(third, FocusTarget::Input);
        let back = third.next();
        assert_eq!(back, FocusTarget::ChatList);
    }

    /// Ensures focus_prev moves backwards through the panel order.
    #[test]
    fn focus_prev_cycles_backwards() {
        let start = FocusTarget::ChatList;
        let prev = start.prev();
        assert_eq!(prev, FocusTarget::Input);
        let prev2 = prev.prev();
        assert_eq!(prev2, FocusTarget::ChatView);
    }

    /// Verifies next() and prev() are inverses for every FocusTarget variant.
    #[test]
    fn focus_next_and_prev_are_inverse() {
        for target in [
            FocusTarget::ChatList,
            FocusTarget::ChatView,
            FocusTarget::Input,
        ] {
            assert_eq!(target.next().prev(), target);
            assert_eq!(target.prev().next(), target);
        }
    }
}
