//! Terminal event handling for the TUI.
//!
//! Provides an [`EventHandler`] that spawns a background task to poll
//! terminal events from crossterm and delivers them via a channel.

use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};
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
                        // Only forward press and repeat events; release events
                        // can carry unexpected KeyCode values on terminals that
                        // support keyboard enhancement protocols (e.g. Kitty,
                        // WezTerm), causing spurious characters to be inserted.
                        CrosstermEvent::Key(key)
                            if key.kind == KeyEventKind::Press
                                || key.kind == KeyEventKind::Repeat =>
                        {
                            Some(Event::Key(key))
                        }
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
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    #[test]
    fn test_should_create_event_variants() {
        let key_event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let evt = Event::Key(key_event);
        assert!(matches!(evt, Event::Key(_)));

        let tick = Event::Tick;
        assert!(matches!(tick, Event::Tick));

        let resize = Event::Resize;
        assert!(matches!(resize, Event::Resize));
    }

    /// Verify that only Press and Repeat key events are forwarded; Release
    /// events must be discarded to prevent spurious characters (e.g. 'h')
    /// being inserted when pressing arrow keys on terminals that emit both
    /// press and release events via the keyboard enhancement protocol.
    #[test]
    fn test_should_accept_press_and_repeat_key_events() {
        let press = KeyEvent::new_with_kind(KeyCode::Left, KeyModifiers::NONE, KeyEventKind::Press);
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Left, KeyModifiers::NONE, KeyEventKind::Repeat);
        let release =
            KeyEvent::new_with_kind(KeyCode::Left, KeyModifiers::NONE, KeyEventKind::Release);

        // The match arm in EventHandler uses the same predicate.
        let accepts =
            |key: &KeyEvent| key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat;

        assert!(accepts(&press), "Press should be accepted");
        assert!(accepts(&repeat), "Repeat should be accepted");
        assert!(!accepts(&release), "Release must be rejected");
    }
}
