//! Terminal event handling for the TUI.
//!
//! Provides an [`EventHandler`] that spawns a background task to poll
//! terminal events from crossterm and delivers them via a channel.

use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use tokio::sync::mpsc;

/// Events delivered by the [`EventHandler`].
#[derive(Debug, Clone)]
pub enum Event {
    /// A keyboard event.
    Key(KeyEvent),
    /// A periodic tick (used to drive redraw cycles).
    Tick,
    /// Terminal was resized.
    Resize,
}

/// Polls crossterm events on a background thread and forwards them
/// through a Tokio MPSC channel.
///
/// The handler automatically stops when the receiver is dropped.
#[derive(Debug)]
pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    _handle: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate in milliseconds.
    ///
    /// Spawns a blocking task that polls terminal events at the configured
    /// tick rate. Keyboard and resize events are forwarded immediately;
    /// a [`Event::Tick`] is sent when no event arrives within the tick
    /// interval.
    pub fn new(tick_rate_ms: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate_ms);
        let (tx, rx) = mpsc::unbounded_channel();

        let handle = tokio::task::spawn_blocking(move || {
            loop {
                // Check if receiver has been dropped.
                if tx.is_closed() {
                    break;
                }

                // Poll crossterm for an event within the tick interval.
                let has_event = match event::poll(tick_rate) {
                    Ok(ready) => ready,
                    Err(_) => {
                        // Terminal error — stop polling.
                        break;
                    }
                };

                if has_event {
                    let evt = match event::read() {
                        Ok(e) => e,
                        Err(_) => break,
                    };

                    let mapped = match evt {
                        CrosstermEvent::Key(key) => Some(Event::Key(key)),
                        CrosstermEvent::Resize(_, _) => Some(Event::Resize),
                        _ => None,
                    };

                    if let Some(e) = mapped
                        && tx.send(e).is_err()
                    {
                        break;
                    }
                } else {
                    // No event within tick window — send a tick.
                    if tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Wait for the next event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event channel is closed (background task stopped).
    pub async fn next(&mut self) -> anyhow::Result<Event> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("event channel closed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_create_event_variants() {
        let key_event = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        );
        let evt = Event::Key(key_event);
        assert!(matches!(evt, Event::Key(_)));

        let tick = Event::Tick;
        assert!(matches!(tick, Event::Tick));

        let resize = Event::Resize;
        assert!(matches!(resize, Event::Resize));
    }
}
