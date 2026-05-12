//! GBA Core - Execute engine for Geektime Bootcamp Agent.
//!
//! This crate provides the core execution engine that wraps the Claude Agent SDK,
//! allowing users to easily add new features around a repository.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

pub mod context;
pub mod engine;
pub mod error;
pub mod preset;
pub mod runner;
pub mod state;
pub mod types;

pub use context::{GbaConfig, GbaContext, PresetOverride};
pub use engine::init::InitEngine;
pub use error::GbaCoreError;
pub use preset::{PresetKind, TaskPreset};
pub use runner::{AgentResult, AgentRunner};
pub use state::{FeatureInfo, FeatureState, PlanState, PrState, TaskState, TotalsState};
pub use types::{
    Feature, FeatureStatus, InitResult, PlanResult, RunResult, TaskKind, TaskProgress, TaskStatus,
};
