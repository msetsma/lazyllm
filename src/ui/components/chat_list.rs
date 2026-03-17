use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::event::types::Action;

use super::Component;

/// Sidebar panel showing the list of conversations.
#[derive(Debug, Clone)]
pub struct ChatList {
    pub items: Vec<String>,
    pub state: ListState,
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
    pub fn update_item(&self, index: usize, title: String) -> Self {
        if index >= self.items.len() {
            return self.clone();
        }
        let mut items = self.items.clone();
        items[index] = title;
        Self {
            items,
            state: self.state.clone(),
        }
    }

    pub fn add_chat(&self, title: String) -> Self {
        let mut items = self.items.clone();
        items.push(title);
        let mut state = self.state.clone();
        state.select(Some(items.len() - 1));
        Self { items, state }
    }

    pub fn remove_selected(&self) -> Self {
        let selected = match self.state.selected() {
            Some(i) if i < self.items.len() => i,
            _ => return self.clone(),
        };
        let mut items = self.items.clone();
        items.remove(selected);
        let new_selected = if items.is_empty() {
            None
        } else if selected >= items.len() {
            Some(items.len() - 1)
        } else {
            Some(selected)
        };
        let mut state = self.state.clone();
        state.select(new_selected);
        Self { items, state }
    }

    fn select_next(&self) -> Self {
        let selected = self.state.selected().unwrap_or(0);
        let next = if selected + 1 < self.items.len() {
            selected + 1
        } else {
            0
        };
        let mut state = self.state.clone();
        state.select(Some(next));
        Self {
            items: self.items.clone(),
            state,
        }
    }

    fn select_prev(&self) -> Self {
        let selected = self.state.selected().unwrap_or(0);
        let prev = if selected == 0 {
            self.items.len().saturating_sub(1)
        } else {
            selected - 1
        };
        let mut state = self.state.clone();
        state.select(Some(prev));
        Self {
            items: self.items.clone(),
            state,
        }
    }
}

impl Component for ChatList {
    fn handle_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ScrollDown => {
                *self = self.select_next();
                None
            }
            Action::ScrollUp => {
                *self = self.select_prev();
                None
            }
            Action::NewChat => {
                let title = format!("Chat {}", self.items.len());
                *self = self.add_chat(title);
                None
            }
            Action::DeleteChat => {
                *self = self.remove_selected();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };

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
                    .border_style(Style::default().fg(border_color)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
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
    fn add_chat_returns_new_list_with_item() {
        let list = ChatList::new();
        let updated = list.add_chat("My Chat".to_string());
        assert_eq!(updated.items.len(), 2);
        assert_eq!(updated.items[1], "My Chat");
        assert_eq!(updated.selected_index(), Some(1));
        // Original unchanged (immutability)
        assert_eq!(list.items.len(), 1);
    }

    #[test]
    fn remove_selected_returns_new_list_without_item() {
        let list = ChatList::new();
        let with_extra = list.add_chat("Second".to_string());
        let removed = with_extra.remove_selected();
        assert_eq!(removed.items.len(), 1);
        assert_eq!(removed.items[0], "New Chat");
    }

    #[test]
    fn remove_selected_allows_removing_last_item() {
        let list = ChatList::new();
        let removed = list.remove_selected();
        assert!(removed.items.is_empty());
        assert_eq!(removed.selected_index(), None);
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
        let list = ChatList::from_items(vec!["Old".to_string(), "Other".to_string()]);
        let updated = list.update_item(0, "New".to_string());
        assert_eq!(updated.items[0], "New");
        assert_eq!(updated.items[1], "Other");
        // Original unchanged
        assert_eq!(list.items[0], "Old");
    }

    #[test]
    fn update_item_out_of_bounds_is_noop() {
        let list = ChatList::from_items(vec!["A".to_string()]);
        let updated = list.update_item(5, "B".to_string());
        assert_eq!(updated.items, list.items);
    }

    #[test]
    fn select_next_wraps_around() {
        let list = ChatList::new()
            .add_chat("A".to_string())
            .add_chat("B".to_string());
        // Currently selected: last item (index 2)
        let next = list.select_next();
        assert_eq!(next.selected_index(), Some(0)); // wraps
    }

    #[test]
    fn select_prev_wraps_around() {
        let mut list = ChatList::new();
        list.state.select(Some(0));
        let prev = list.select_prev();
        assert_eq!(prev.selected_index(), Some(0)); // only 1 item, stays
    }

    #[test]
    fn handle_action_scroll_down_selects_next() {
        let mut list = ChatList::new();
        list = list.add_chat("Second".to_string());
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
