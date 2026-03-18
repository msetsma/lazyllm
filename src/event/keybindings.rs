use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::types::{Action, FocusTarget, Mode};

/// Resolves a key event into an action based on the current mode and focus.
pub fn resolve_key(key: KeyEvent, mode: Mode, focus: FocusTarget) -> Action {
    // Global keybindings (work in any mode)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Action::Quit,
            _ => Action::None,
        };
    }

    match mode {
        Mode::Insert => resolve_insert_mode(key, focus),
        Mode::Normal => resolve_normal_mode(key, focus),
        Mode::Visual => resolve_visual_mode(key),
        Mode::Command => resolve_command_mode(key),
        Mode::Search => resolve_search_mode(key),
    }
}

fn resolve_insert_mode(key: KeyEvent, _focus: FocusTarget) -> Action {
    match key.code {
        KeyCode::Esc => Action::SwitchMode(Mode::Normal),
        KeyCode::Enter => Action::SendMessage,
        KeyCode::Backspace => Action::DeleteChar,
        KeyCode::Char(c) => Action::InsertChar(c),
        _ => Action::None,
    }
}

fn resolve_normal_mode(key: KeyEvent, _focus: FocusTarget) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('i') => Action::SwitchMode(Mode::Insert),
        KeyCode::Char('v') => Action::SwitchMode(Mode::Visual),
        KeyCode::Char(':') => Action::SwitchMode(Mode::Command),
        KeyCode::Char('/') => Action::SwitchMode(Mode::Search),
        KeyCode::Tab => Action::FocusNext,
        KeyCode::BackTab => Action::FocusPrev,
        KeyCode::Char('j') | KeyCode::Down => Action::ScrollDown,
        KeyCode::Char('k') | KeyCode::Up => Action::ScrollUp,
        KeyCode::Enter => Action::SelectItem,
        KeyCode::Char('n') => Action::NewChat,
        KeyCode::Char('d') => Action::DeleteChat,
        KeyCode::Char('m') => Action::ToggleModelSelector,
        KeyCode::Char('?') => Action::ToggleHelp,
        KeyCode::Char('l') | KeyCode::Right => Action::FocusNext,
        KeyCode::Char('h') | KeyCode::Left => Action::FocusPrev,
        _ => Action::None,
    }
}

fn resolve_search_mode(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::SwitchMode(Mode::Normal),
        KeyCode::Enter => Action::SearchNext,
        KeyCode::Backspace => Action::DeleteChar,
        KeyCode::Char(c) => Action::InsertChar(c),
        KeyCode::Down => Action::SearchNext,
        KeyCode::Up => Action::SearchPrev,
        _ => Action::None,
    }
}

fn resolve_visual_mode(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::SwitchMode(Mode::Normal),
        KeyCode::Char('j') | KeyCode::Down => Action::ScrollDown,
        KeyCode::Char('k') | KeyCode::Up => Action::ScrollUp,
        KeyCode::Char('y') => Action::CopySelection,
        _ => Action::None,
    }
}

fn resolve_command_mode(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::SwitchMode(Mode::Normal),
        KeyCode::Enter => Action::InputSubmit,
        KeyCode::Backspace => Action::DeleteChar,
        KeyCode::Char(c) => Action::InsertChar(c),
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_mods(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn ctrl_c_quits_in_any_mode() {
        let ctrl_c = key_with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL);
        for mode in [Mode::Normal, Mode::Insert, Mode::Visual, Mode::Command] {
            assert_eq!(
                resolve_key(ctrl_c, mode, FocusTarget::ChatView),
                Action::Quit,
                "Ctrl+C should quit in {:?} mode",
                mode
            );
        }
    }

    #[test]
    fn normal_mode_q_quits() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('q')), Mode::Normal, FocusTarget::ChatView),
            Action::Quit
        );
    }

    #[test]
    fn normal_mode_i_enters_insert() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('i')), Mode::Normal, FocusTarget::ChatView),
            Action::SwitchMode(Mode::Insert)
        );
    }

    #[test]
    fn normal_mode_tab_focuses_next() {
        assert_eq!(
            resolve_key(key(KeyCode::Tab), Mode::Normal, FocusTarget::ChatView),
            Action::FocusNext
        );
    }

    #[test]
    fn normal_mode_backtab_focuses_prev() {
        assert_eq!(
            resolve_key(key(KeyCode::BackTab), Mode::Normal, FocusTarget::ChatView),
            Action::FocusPrev
        );
    }

    #[test]
    fn normal_mode_jk_scrolls() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('j')), Mode::Normal, FocusTarget::ChatView),
            Action::ScrollDown
        );
        assert_eq!(
            resolve_key(key(KeyCode::Char('k')), Mode::Normal, FocusTarget::ChatView),
            Action::ScrollUp
        );
    }

    #[test]
    fn insert_mode_esc_returns_to_normal() {
        assert_eq!(
            resolve_key(key(KeyCode::Esc), Mode::Insert, FocusTarget::Input),
            Action::SwitchMode(Mode::Normal)
        );
    }

    #[test]
    fn insert_mode_typing_produces_insert_char() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('a')), Mode::Insert, FocusTarget::Input),
            Action::InsertChar('a')
        );
    }

    #[test]
    fn insert_mode_enter_sends_message() {
        assert_eq!(
            resolve_key(key(KeyCode::Enter), Mode::Insert, FocusTarget::Input),
            Action::SendMessage
        );
    }

    #[test]
    fn insert_mode_backspace_deletes() {
        assert_eq!(
            resolve_key(key(KeyCode::Backspace), Mode::Insert, FocusTarget::Input),
            Action::DeleteChar
        );
    }

    #[test]
    fn visual_mode_esc_returns_to_normal() {
        assert_eq!(
            resolve_key(key(KeyCode::Esc), Mode::Visual, FocusTarget::ChatView),
            Action::SwitchMode(Mode::Normal)
        );
    }

    #[test]
    fn command_mode_esc_returns_to_normal() {
        assert_eq!(
            resolve_key(key(KeyCode::Esc), Mode::Command, FocusTarget::ChatView),
            Action::SwitchMode(Mode::Normal)
        );
    }

    #[test]
    fn command_mode_enter_submits() {
        assert_eq!(
            resolve_key(key(KeyCode::Enter), Mode::Command, FocusTarget::ChatView),
            Action::InputSubmit
        );
    }

    #[test]
    fn normal_mode_question_mark_toggles_help() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('?')), Mode::Normal, FocusTarget::ChatView),
            Action::ToggleHelp
        );
    }

    #[test]
    fn normal_mode_n_creates_new_chat() {
        assert_eq!(
            resolve_key(key(KeyCode::Char('n')), Mode::Normal, FocusTarget::ChatList),
            Action::NewChat
        );
    }

    #[test]
    fn unbound_keys_produce_none() {
        assert_eq!(
            resolve_key(key(KeyCode::F(12)), Mode::Normal, FocusTarget::ChatView),
            Action::None
        );
    }
}
