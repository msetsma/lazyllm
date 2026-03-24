use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::event::types::Action;
use crate::llm::context::ContextBudget;
use crate::store::types::CompactionEvent;
use crate::ui::theme::Theme;

use super::centered_rect;

/// Pulse overlay showing context budget, session stats, pins, notes, and compaction history.
#[derive(Debug, Clone)]
pub struct PulseOverlay {
    pub(crate) visible: bool,
    pub(crate) scroll_offset: u16,
    budget: ContextBudget,
    stats: PulseStats,
    pinned_previews: Vec<PinnedPreview>,
    session_notes: String,
    compaction_events: Vec<CompactionEvent>,
    compaction_mode: String,
}

/// Session statistics for the Pulse overlay.
#[derive(Debug, Clone, Default)]
pub struct PulseStats {
    pub turn_count: u32,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_cost: f64,
    pub cache_hit_rate: Option<f32>,
    pub est_cost_per_message: f64,
    pub compaction_count: usize,
    pub dropped_message_count: usize,
}

/// Preview of a pinned message for display.
#[derive(Debug, Clone)]
pub struct PinnedPreview {
    pub message_index: usize,
    pub role: String,
    pub preview: String,
}

impl Default for PulseOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl PulseOverlay {
    pub fn new() -> Self {
        Self {
            visible: false,
            scroll_offset: 0,
            budget: ContextBudget::default(),
            stats: PulseStats::default(),
            pinned_previews: Vec::new(),
            session_notes: String::new(),
            compaction_events: Vec::new(),
            compaction_mode: "auto".to_string(),
        }
    }

    /// Refresh all overlay data. Called when the overlay is opened.
    pub fn refresh(
        &mut self,
        budget: ContextBudget,
        stats: PulseStats,
        pinned_previews: Vec<PinnedPreview>,
        session_notes: String,
        compaction_events: Vec<CompactionEvent>,
        compaction_mode: String,
    ) {
        self.budget = budget;
        self.stats = stats;
        self.pinned_previews = pinned_previews;
        self.session_notes = session_notes;
        self.compaction_events = compaction_events;
        self.compaction_mode = compaction_mode;
        self.scroll_offset = 0;
    }

    /// Handle a raw key event when the overlay is visible.
    /// Returns Some(action) if consumed, None if not.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('P') => {
                Some(Action::TogglePulse)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                Some(Action::Tick)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                Some(Action::Tick)
            }
            KeyCode::Char('c') => Some(Action::PulseCompact),
            KeyCode::Char('C') => Some(Action::PulseCompactWithPrompt),
            KeyCode::Char('t') => Some(Action::PulseClearToolResults),
            KeyCode::Char('u') => Some(Action::PulseUndoCompaction),
            KeyCode::Char('p') => Some(Action::PulseTogglePin),
            KeyCode::Char('n') => Some(Action::PulseEditNotes),
            KeyCode::Char('s') => Some(Action::PulseSwitchMode),
            _ => Some(Action::Tick) // consume all keys when overlay is visible
        }
    }

    fn build_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // --- Context Budget ---
        lines.push(section_title("Context budget", theme));

        let pct = (self.budget.usage_fraction * 100.0) as u32;
        let total_k = self.budget.total_budget / 1000;
        let window_k = self.budget.context_window / 1000;
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}% of {}k budget ({}k window)", pct, total_k, window_k),
                Style::default().fg(theme.health_color(&self.budget.health)),
            ),
        ]));

        // Budget breakdown
        lines.push(budget_segment(
            "system", self.budget.system_prompt_tokens, theme.budget_system,
        ));
        lines.push(budget_segment(
            "context", self.budget.context_files_tokens, theme.budget_context,
        ));
        lines.push(budget_segment(
            "tools", self.budget.tool_definitions_tokens, theme.budget_tools,
        ));
        if self.budget.compaction_summary_tokens > 0 {
            lines.push(budget_segment(
                "summary", self.budget.compaction_summary_tokens, theme.budget_compaction,
            ));
        }
        if self.budget.pinned_messages_tokens > 0 {
            lines.push(budget_segment(
                "pinned", self.budget.pinned_messages_tokens, theme.pulse_pin_icon,
            ));
        }
        lines.push(budget_segment(
            "messages", self.budget.message_history_tokens, theme.budget_messages,
        ));
        lines.push(budget_segment(
            "free", self.budget.free_tokens, theme.budget_free,
        ));
        lines.push(Line::from(""));

        // --- Session Stats ---
        lines.push(section_title("Session", theme));
        lines.push(stat_line("turns", &self.stats.turn_count.to_string()));
        lines.push(stat_line(
            "in",
            &format!("{}k", self.stats.total_input_tokens / 1000),
        ));
        lines.push(stat_line(
            "out",
            &format!("{}k", self.stats.total_output_tokens / 1000),
        ));
        lines.push(stat_line(
            "cost",
            &format!("${:.2}", self.stats.total_cost),
        ));
        if let Some(rate) = self.stats.cache_hit_rate {
            lines.push(stat_line("cache", &format!("{:.0}%", rate * 100.0)));
        }
        lines.push(stat_line(
            "~$/msg",
            &format!("${:.4}", self.stats.est_cost_per_message),
        ));
        lines.push(stat_line(
            "compactions",
            &self.stats.compaction_count.to_string(),
        ));
        lines.push(stat_line(
            "dropped",
            &self.stats.dropped_message_count.to_string(),
        ));
        lines.push(stat_line("mode", &self.compaction_mode));
        lines.push(Line::from(""));

        // --- Pinned Messages ---
        if !self.pinned_previews.is_empty() {
            lines.push(section_title("Pinned (survive compaction)", theme));
            for pin in &self.pinned_previews {
                lines.push(Line::from(vec![
                    Span::styled(
                        "  \u{1F4CC} ",
                        Style::default().fg(theme.pulse_pin_icon),
                    ),
                    Span::styled(
                        format!("#{} ", pin.message_index),
                        Style::default().fg(theme.label),
                    ),
                    Span::raw(pin.preview.clone()),
                ]));
            }
            lines.push(Line::from(""));
        }

        // --- Session Notes ---
        if !self.session_notes.is_empty() {
            lines.push(section_title("Session notes", theme));
            for note_line in self.session_notes.lines() {
                lines.push(Line::from(vec![
                    Span::styled(
                        "  \u{2503} ",
                        Style::default().fg(theme.pulse_note_border),
                    ),
                    Span::raw(note_line.to_string()),
                ]));
            }
            lines.push(Line::from(""));
        }

        // --- Compaction History ---
        if !self.compaction_events.is_empty() {
            lines.push(section_title("Compaction history", theme));
            for event in self.compaction_events.iter().rev().take(5) {
                let time = event.timestamp.format("%H:%M");
                let ckpt = if event.checkpoint_id.is_some() {
                    " ckpt\u{2713}"
                } else {
                    ""
                };
                lines.push(Line::from(format!(
                    "  {} {:?}  {} msgs \u{2192} {}k summary{}",
                    time,
                    event.mode,
                    event.messages_before,
                    event.tokens_reclaimed / 1000,
                    ckpt,
                )));
            }
            lines.push(Line::from(""));
        }

        // --- Key hints ---
        lines.push(Line::from(vec![
            Span::styled("  c", Style::default().fg(theme.help_key)),
            Span::raw(" compact  "),
            Span::styled("C", Style::default().fg(theme.help_key)),
            Span::raw(" compact+prompt  "),
            Span::styled("t", Style::default().fg(theme.help_key)),
            Span::raw(" clear tools"),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  u", Style::default().fg(theme.help_key)),
            Span::raw(" undo  "),
            Span::styled("p", Style::default().fg(theme.help_key)),
            Span::raw(" pin  "),
            Span::styled("n", Style::default().fg(theme.help_key)),
            Span::raw(" notes  "),
            Span::styled("s", Style::default().fg(theme.help_key)),
            Span::raw(" mode  "),
            Span::styled("Esc", Style::default().fg(theme.help_key)),
            Span::raw(" close"),
        ]));

        lines
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, _focused: bool, theme: &Theme) {
        if !self.visible {
            return;
        }

        let lines = self.build_lines(theme);
        let content_height = lines.len() as u16 + 2;
        let width = (area.width * 88 / 100).min(90).max(50);
        let height = content_height.min(area.height.saturating_sub(4));

        let popup_area = centered_rect(width, height, area);
        frame.render_widget(Clear, popup_area);

        let health_label = format!(
            " {} {}% ",
            self.budget.health.label(),
            (self.budget.usage_fraction * 100.0) as u32,
        );

        let title = format!(" \u{25C9} Pulse ");

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .title_bottom(health_label)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.pulse_border)),
            )
            .scroll((self.scroll_offset, 0))
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, popup_area);
    }
}

fn section_title(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {title}"),
        Style::default()
            .fg(theme.pulse_section_title)
            .add_modifier(Modifier::BOLD),
    ))
}

fn budget_segment(label: &str, tokens: u32, color: ratatui::style::Color) -> Line<'static> {
    let k = if tokens >= 1000 {
        format!("{}k", tokens / 1000)
    } else {
        format!("{tokens}")
    };
    Line::from(vec![
        Span::styled(
            format!("    \u{25A0} {label:<10}"),
            Style::default().fg(color),
        ),
        Span::raw(k),
    ])
}

fn stat_line(label: &str, value: &str) -> Line<'static> {
    Line::from(format!("    {label:<14}{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_overlay_starts_hidden() {
        let overlay = PulseOverlay::new();
        assert!(!overlay.visible);
    }

    #[test]
    fn handle_key_esc_closes() {
        let mut overlay = PulseOverlay::new();
        overlay.visible = true;
        let action = overlay.handle_key(KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE));
        assert_eq!(action, Some(Action::TogglePulse));
    }

    #[test]
    fn handle_key_c_compacts() {
        let mut overlay = PulseOverlay::new();
        overlay.visible = true;
        let action = overlay.handle_key(KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(action, Some(Action::PulseCompact));
    }

    #[test]
    fn handle_key_j_scrolls_down() {
        let mut overlay = PulseOverlay::new();
        overlay.visible = true;
        assert_eq!(overlay.scroll_offset, 0);
        overlay.handle_key(KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(overlay.scroll_offset, 1);
    }

    #[test]
    fn handle_key_k_scrolls_up_clamped() {
        let mut overlay = PulseOverlay::new();
        overlay.visible = true;
        // Already at 0, should stay at 0
        overlay.handle_key(KeyEvent::new(KeyCode::Char('k'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(overlay.scroll_offset, 0);
    }

    #[test]
    fn refresh_resets_scroll() {
        let mut overlay = PulseOverlay::new();
        overlay.scroll_offset = 5;
        overlay.refresh(
            ContextBudget::default(),
            PulseStats::default(),
            vec![],
            String::new(),
            vec![],
            "auto".into(),
        );
        assert_eq!(overlay.scroll_offset, 0);
    }

    #[test]
    fn build_lines_contains_sections() {
        let overlay = PulseOverlay::new();
        let theme = Theme::default();
        let lines = overlay.build_lines(&theme);
        let text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("Context budget"));
        assert!(text.contains("Session"));
        assert!(text.contains("compact"));
    }

    #[test]
    fn build_lines_includes_pinned_when_present() {
        let mut overlay = PulseOverlay::new();
        overlay.pinned_previews = vec![PinnedPreview {
            message_index: 3,
            role: "user".into(),
            preview: "important message".into(),
        }];
        let theme = Theme::default();
        let lines = overlay.build_lines(&theme);
        let text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("Pinned"));
        assert!(text.contains("important message"));
    }

    #[test]
    fn build_lines_includes_notes_when_present() {
        let mut overlay = PulseOverlay::new();
        overlay.session_notes = "my note".into();
        let theme = Theme::default();
        let lines = overlay.build_lines(&theme);
        let text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("Session notes"));
        assert!(text.contains("my note"));
    }
}
