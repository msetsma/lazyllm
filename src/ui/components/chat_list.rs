use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::event::types::Action;
use crate::ui::theme::Theme;

use super::Component;

/// Sidebar panel showing the list of conversations.
#[derive(Debug, Clone)]
pub struct ChatList {
    pub(crate) items: Vec<String>,
    pub(crate) state: ListState,
}

impl Default for ChatList {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatList {
    pub fn new() -> Self {
        Self {
            items: vec!["New Chat".to_string()],
            state: ListState::default().with_selected(Some(0)),
        }
    }

    /// Create a chat list from existing items.
    pub fn from_items(items: Vec<String>) -> Self {
        let selected = if items.is_empty() { None } else { Some(0) };
        Self {
            items,
            state: ListState::default().with_selected(selected),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    /// Update the title of an item at the given index.
    pub fn update_item(&mut self, index: usize, title: String) {
        if index < self.items.len() {
            self.items[index] = title;
        }
    }

    pub fn add_chat(&mut self, title: String) {
        self.items.push(title);
        self.state.select(Some(self.items.len() - 1));
    }

    pub fn remove_selected(&mut self) {
        let selected = match self.state.selected() {
            Some(i) if i < self.items.len() => i,
            _ => return,
        };
        self.items.remove(selected);
        let new_selected = if self.items.is_empty() {
            None
        } else if selected >= self.items.len() {
            Some(self.items.len() - 1)
        } else {
            Some(selected)
        };
        self.state.select(new_selected);
    }

    fn select_next(&mut self) {
        let selected = self.state.selected().unwrap_or(0);
        let next = if selected + 1 < self.items.len() {
            selected + 1
        } else {
            0
        };
        self.state.select(Some(next));
    }

    fn select_prev(&mut self) {
        let selected = self.state.selected().unwrap_or(0);
        let prev = if selected == 0 {
            self.items.len().saturating_sub(1)
        } else {
            selected - 1
        };
        self.state.select(Some(prev));
    }
}

impl Component for ChatList {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ScrollDown => {
                self.select_next();
                None
            }
            Action::ScrollUp => {
                self.select_prev();
                None
            }
            Action::NewChat => {
                let title = format!("Chat {}", self.items.len());
                self.add_chat(title);
                None
            }
            Action::DeleteChat => {
                self.remove_selected();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = super::focused_border_style(focused, theme);

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|title| ListItem::new(Line::from(title.as_str())))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Chats ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(
                Style::default()
                    .fg(theme.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_chat_list_has_one_item() {
        let list = ChatList::new();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0], "New Chat");
        assert_eq!(list.selected_index(), Some(0));
    }

    #[test]
    fn add_chat_appends_item() {
        let mut list = ChatList::new();
        list.add_chat("My Chat".to_string());
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[1], "My Chat");
        assert_eq!(list.selected_index(), Some(1));
    }

    #[test]
    fn remove_selected_removes_item() {
        let mut list = ChatList::new();
        list.add_chat("Second".to_string());
        list.remove_selected();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0], "New Chat");
    }

    #[test]
    fn remove_selected_allows_removing_last_item() {
        let mut list = ChatList::new();
        list.remove_selected();
        assert!(list.items.is_empty());
        assert_eq!(list.selected_index(), None);
    }

    #[test]
    fn from_items_creates_list() {
        let list = ChatList::from_items(vec!["A".to_string(), "B".to_string()]);
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.selected_index(), Some(0));
    }

    #[test]
    fn from_items_empty_has_no_selection() {
        let list = ChatList::from_items(vec![]);
        assert!(list.items.is_empty());
        assert_eq!(list.selected_index(), None);
    }

    #[test]
    fn update_item_changes_title() {
        let mut list = ChatList::from_items(vec!["Old".to_string(), "Other".to_string()]);
        list.update_item(0, "New".to_string());
        assert_eq!(list.items[0], "New");
        assert_eq!(list.items[1], "Other");
    }

    #[test]
    fn update_item_out_of_bounds_is_noop() {
        let mut list = ChatList::from_items(vec!["A".to_string()]);
        list.update_item(5, "B".to_string());
        assert_eq!(list.items, vec!["A".to_string()]);
    }

    #[test]
    fn select_next_wraps_around() {
        let mut list = ChatList::new();
        list.add_chat("A".to_string());
        list.add_chat("B".to_string());
        // Currently selected: last item (index 2)
        list.select_next();
        assert_eq!(list.selected_index(), Some(0)); // wraps
    }

    #[test]
    fn select_prev_wraps_around() {
        let mut list = ChatList::new();
        list.state.select(Some(0));
        list.select_prev();
        assert_eq!(list.selected_index(), Some(0)); // only 1 item, stays
    }

    #[test]
    fn handle_action_scroll_down_selects_next() {
        let mut list = ChatList::new();
        list.add_chat("Second".to_string());
        list.state.select(Some(0));
        list.handle_action(&Action::ScrollDown);
        assert_eq!(list.selected_index(), Some(1));
    }

    #[test]
    fn handle_action_new_chat_adds_item() {
        let mut list = ChatList::new();
        list.handle_action(&Action::NewChat);
        assert_eq!(list.items.len(), 2);
    }

    #[test]
    fn handle_action_delete_chat_removes_item() {
        let mut list = ChatList::new();
        list.handle_action(&Action::NewChat);
        assert_eq!(list.items.len(), 2);
        list.handle_action(&Action::DeleteChat);
        assert_eq!(list.items.len(), 1);
    }
}
