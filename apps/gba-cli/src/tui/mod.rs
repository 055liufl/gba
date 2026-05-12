//! TUI (Terminal User Interface) module for the GBA CLI.
//!
//! Provides an interactive chat-style interface for the `gba plan` command
//! using [`ratatui`] and [`crossterm`]. The module is structured as:
//!
//! - [`app`] — Application state (chat history, input, state machine)
//! - [`event`] — Background terminal event polling
//! - [`ui`] — Rendering logic (layout, widgets, colors)

pub mod app;
pub mod event;
pub mod ui;
