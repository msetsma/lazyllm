use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::event::types::Action;
use crate::llm::ProviderRegistry;
use crate::ui::theme::Theme;

use super::{Component, centered_rect};

/// Overlay popup for selecting a model/provider combination.
#[derive(Debug, Clone, Default)]
pub struct ModelPopup {
    pub(crate) visible: bool,
    /// Flat list of (provider_name, model_id) entries.
    pub(crate) entries: Vec<(String, String)>,
    pub(crate) list_state: ListState,
    /// Currently active (provider, model) — used to mark the active entry.
    pub(crate) active: Option<(String, String)>,
}

impl ModelPopup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the popup, building the entry list from the registry.
    pub fn open(&mut self, registry: &ProviderRegistry, active_provider: &str, active_model: &str) {
        let mut entries = Vec::new();
        let mut providers = registry.list_providers();
        providers.sort();
        let mut active_idx = None;
        for provider_name in providers {
            if let Some(provider) = registry.get(provider_name) {
                for model in provider.available_models() {
                    if provider_name == active_provider && model.id == active_model {
                        active_idx = Some(entries.len());
                    }
                    entries.push((provider_name.to_string(), model.id.clone()));
                }
            }
        }
        self.entries = entries;
        self.active = Some((active_provider.to_string(), active_model.to_string()));
        self.visible = true;
        // Pre-select the active model, or first entry
        if !self.entries.is_empty() {
            self.list_state.select(Some(active_idx.unwrap_or(0)));
        } else {
            self.list_state.select(None);
        }
    }

    /// Close the popup.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move selection down with wrap-around.
    pub fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current + 1 < self.entries.len() {
            current + 1
        } else {
            0
        };
        self.list_state.select(Some(next));
    }

    /// Move selection up with wrap-around.
    pub fn select_prev(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = if current == 0 {
            self.entries.len().saturating_sub(1)
        } else {
            current - 1
        };
        self.list_state.select(Some(prev));
    }

    /// Return the currently selected (provider, model) entry.
    pub fn selected_entry(&self) -> Option<(String, String)> {
        let idx = self.list_state.selected()?;
        self.entries.get(idx).cloned()
    }
}

impl Component for ModelPopup {
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
            Action::SelectItem => {
                if self.selected_entry().is_some() {
                    Some(Action::SelectModel)
                } else {
                    None
                }
            }
            Action::ToggleModelSelector => {
                self.close();
                None
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        if !self.visible {
            return;
        }

        let height = (self.entries.len() as u16 + 4).min(area.height.saturating_sub(4));
        let width = 50.min(area.width.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);
        frame.render_widget(Clear, popup_area);

        if self.entries.is_empty() {
            let block = Block::default()
                .title(" Select Model ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.popup_border));
            let item = ListItem::new("No providers configured");
            let list = List::new(vec![item]).block(block);
            frame.render_widget(list, popup_area);
            return;
        }

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|(provider, model)| {
                let is_active = self
                    .active
                    .as_ref()
                    .is_some_and(|(ap, am)| ap == provider && am == model);
                let label = if is_active {
                    format!("{provider} / {model} *")
                } else {
                    format!("{provider} / {model}")
                };
                ListItem::new(label)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Select Model ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.popup_border)),
            )
            .highlight_style(
                Style::default()
                    .fg(theme.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        let mut state = self.list_state;
        frame.render_stateful_widget(list, popup_area, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_popup() -> ModelPopup {
        ModelPopup::new()
    }

    #[test]
    fn starts_hidden_and_empty() {
        let popup = empty_popup();
        assert!(!popup.visible);
        assert!(popup.entries.is_empty());
        assert!(popup.selected_entry().is_none());
    }

    #[test]
    fn open_with_empty_registry() {
        let mut popup = empty_popup();
        let registry = ProviderRegistry::new();
        popup.open(&registry, "", "");
        assert!(popup.visible);
        assert!(popup.entries.is_empty());
        assert!(popup.selected_entry().is_none());
    }

    #[test]
    fn close_hides_popup() {
        let mut popup = empty_popup();
        popup.visible = true;
        popup.close();
        assert!(!popup.visible);
    }

    #[test]
    fn select_next_wraps_around() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![
                ("a".into(), "m1".into()),
                ("a".into(), "m2".into()),
                ("b".into(), "m3".into()),
            ],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            active: None,
        };

        popup.select_next();
        assert_eq!(popup.list_state.selected(), Some(1));
        popup.select_next();
        assert_eq!(popup.list_state.selected(), Some(2));
        popup.select_next();
        assert_eq!(popup.list_state.selected(), Some(0)); // wrap
    }

    #[test]
    fn select_prev_wraps_around() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![
                ("a".into(), "m1".into()),
                ("a".into(), "m2".into()),
            ],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            active: None,
        };

        popup.select_prev();
        assert_eq!(popup.list_state.selected(), Some(1)); // wrap
        popup.select_prev();
        assert_eq!(popup.list_state.selected(), Some(0));
    }

    #[test]
    fn select_on_empty_does_nothing() {
        let mut popup = empty_popup();
        popup.select_next();
        popup.select_prev();
        assert!(popup.selected_entry().is_none());
    }

    #[test]
    fn selected_entry_returns_current() {
        let popup = ModelPopup {
            visible: true,
            entries: vec![
                ("openai".into(), "gpt-4o".into()),
                ("anthropic".into(), "claude-sonnet-4-20250514".into()),
            ],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(1));
                s
            },
            active: None,
        };

        let entry = popup.selected_entry().unwrap();
        assert_eq!(entry, ("anthropic".to_string(), "claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn handle_action_scroll_down() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![("a".into(), "m1".into()), ("b".into(), "m2".into())],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            active: None,
        };
        let result = popup.handle_action(&Action::ScrollDown);
        assert!(result.is_none());
        assert_eq!(popup.list_state.selected(), Some(1));
    }

    #[test]
    fn handle_action_scroll_up() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![("a".into(), "m1".into()), ("b".into(), "m2".into())],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(1));
                s
            },
            active: None,
        };
        let result = popup.handle_action(&Action::ScrollUp);
        assert!(result.is_none());
        assert_eq!(popup.list_state.selected(), Some(0));
    }

    #[test]
    fn handle_action_select_item_returns_select_model() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![("openai".into(), "gpt-4o".into())],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            active: None,
        };
        let result = popup.handle_action(&Action::SelectItem);
        assert_eq!(result, Some(Action::SelectModel));
    }

    #[test]
    fn handle_action_toggle_closes() {
        let mut popup = ModelPopup {
            visible: true,
            entries: vec![],
            list_state: ListState::default(),
            active: None,
        };
        popup.handle_action(&Action::ToggleModelSelector);
        assert!(!popup.visible);
    }

    #[test]
    fn active_entry_marked_in_render_label() {
        let popup = ModelPopup {
            visible: true,
            entries: vec![
                ("openai".into(), "gpt-4o".into()),
                ("anthropic".into(), "claude".into()),
            ],
            list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            active: Some(("anthropic".into(), "claude".into())),
        };
        // The active entry should be the second one
        assert_eq!(
            popup.active,
            Some(("anthropic".to_string(), "claude".to_string()))
        );
    }

    #[test]
    fn open_preselects_active_model() {
        use crate::llm::types::ModelInfo;
        use async_trait::async_trait;
        use tokio::sync::mpsc;
        use crate::llm::{LlmProvider, types::{ChatRequest, LlmError, StreamChunk}};

        struct FakeProvider {
            name: String,
            models: Vec<ModelInfo>,
        }
        #[async_trait]
        impl LlmProvider for FakeProvider {
            fn name(&self) -> &str { &self.name }
            fn available_models(&self) -> Vec<ModelInfo> { self.models.clone() }
            async fn chat(&self, _: ChatRequest, _: mpsc::UnboundedSender<StreamChunk>) -> Result<(), LlmError> {
                Ok(())
            }
        }

        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(FakeProvider {
            name: "openai".into(),
            models: vec![ModelInfo::new("gpt-4o"), ModelInfo::new("gpt-4o-mini")],
        }));

        let mut popup = ModelPopup::new();
        popup.open(&registry, "openai", "gpt-4o-mini");

        // Should preselect index 1 (gpt-4o-mini)
        assert_eq!(popup.list_state.selected(), Some(1));
        assert_eq!(
            popup.active,
            Some(("openai".to_string(), "gpt-4o-mini".to_string()))
        );
    }
}
