pub mod keybindings;
pub mod types;

use std::time::Duration;

use crossterm::event::{self, Event};
use tokio::sync::mpsc;

use types::AppEvent;

/// Spawns a background task that polls terminal events and sends them
/// through the provided channel. Returns the join handle.
pub fn spawn_event_loop(
    tx: mpsc::UnboundedSender<AppEvent>,
    tick_rate: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let has_event = tokio::task::spawn_blocking({
                let tick_rate = tick_rate;
                move || event::poll(tick_rate).unwrap_or(false)
            })
            .await
            .unwrap_or(false);

            if has_event {
                let event = tokio::task::spawn_blocking(|| event::read().ok())
                    .await
                    .ok()
                    .flatten();

                if let Some(evt) = event {
                    let app_event = match evt {
                        Event::Key(key) => Some(AppEvent::Key(key)),
                        Event::Resize(w, h) => Some(AppEvent::Resize(w, h)),
                        _ => None,
                    };

                    if let Some(app_event) = app_event {
                        if tx.send(app_event).is_err() {
                            break;
                        }
                    }
                }
            } else {
                // No event within tick_rate, send a tick
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        }
    })
}
