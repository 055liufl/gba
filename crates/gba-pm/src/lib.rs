//! GBA Prompt Manager - Template-based prompt management for GBA.
//!
//! This crate provides prompt management using MiniJinja templates,
//! enabling flexible and reusable prompt composition. All templates
//! are embedded at compile time so the binary has no runtime file
//! dependencies.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

pub mod error;
pub mod manager;
mod templates;

pub use error::GbaPmError;
pub use manager::PromptManager;
