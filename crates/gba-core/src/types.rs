//! Shared domain types for the GBA engine.
//!
//! This module contains enums, structs, and result types used across
//! the core engine, CLI, and state persistence layers.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The kind of task in a feature's implementation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Initial directory / scaffold setup.
    Setup,
    /// Build (implement) a phase.
    Build,
    /// Git commit after a build phase.
    Commit,
    /// Run tests and verification checks.
    Verification,
    /// Code review (read-only).
    Review,
    /// Apply fixes from code review findings.
    ReviewFix,
    /// Push branch and submit a pull request.
    SubmitPr,
}

/// The execution status of a single task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Intentionally skipped.
    Skipped,
}

/// The high-level status of a feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    /// Plan has been generated but execution has not started.
    Planned,
    /// Implementation is in progress.
    Running,
    /// Code review is in progress.
    Reviewing,
    /// All tasks completed and PR submitted.
    Completed,
    /// A task failed and execution stopped.
    Failed,
}

/// A feature reference used in listing and display.
#[derive(Debug, Clone)]
pub struct Feature {
    /// Sequential feature number (1-based).
    pub number: u32,
    /// URL-friendly slug for the feature.
    pub slug: String,
    /// Path to the feature directory under `.gba/`.
    pub path: PathBuf,
}

/// Progress report for a single task, used in callbacks.
#[derive(Debug, Clone)]
pub struct TaskProgress {
    /// Task ID within the feature.
    pub task_id: u32,
    /// Kind of task being reported.
    pub kind: TaskKind,
    /// Human-readable description of the task.
    pub description: String,
    /// Current status.
    pub status: TaskStatus,
}

/// Result of `gba init`.
#[derive(Debug, Clone)]
pub struct InitResult {
    /// Whether initialization was actually performed (false if already initialized).
    pub performed: bool,
    /// Summary text from the repo analysis.
    pub summary: String,
}

/// Result of `gba plan`.
#[derive(Debug, Clone)]
pub struct PlanResult {
    /// Feature number assigned.
    pub feature_number: u32,
    /// Feature slug.
    pub slug: String,
    /// Total turns used during planning.
    pub turns: u32,
    /// Total cost in USD for the planning session.
    pub cost_usd: f64,
}

/// Result of `gba run`.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// URL of the created pull request, if any.
    pub pr_url: String,
    /// Total turns across all tasks.
    pub total_turns: u32,
    /// Total cost in USD across all tasks.
    pub total_cost_usd: f64,
}
