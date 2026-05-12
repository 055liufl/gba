//! Error types for the GBA Core engine.

use std::path::PathBuf;

use thiserror::Error;

/// Errors that can occur in the GBA Core engine.
#[derive(Debug, Error)]
pub enum GbaCoreError {
    /// Failed to load feature state from disk.
    #[error("failed to load state from {path}: {source}")]
    StateLoad {
        /// Path that was being loaded.
        path: PathBuf,
        /// Underlying IO or deserialization error.
        #[source]
        source: anyhow::Error,
    },

    /// Failed to save feature state to disk.
    #[error("failed to save state to {path}: {source}")]
    StateSave {
        /// Path that was being written.
        path: PathBuf,
        /// Underlying IO or serialization error.
        #[source]
        source: anyhow::Error,
    },

    /// Failed to render a prompt template.
    #[error("failed to render prompt '{template}': {source}")]
    PromptRender {
        /// Name of the template that failed.
        template: String,
        /// Underlying prompt manager error.
        #[source]
        source: gba_pm::GbaPmError,
    },

    /// Failed to execute an agent query.
    #[error("agent query failed: {message}")]
    AgentQuery {
        /// Description of what went wrong.
        message: String,
        /// Underlying SDK error, if any.
        #[source]
        source: Option<anyhow::Error>,
    },

    /// Failed to load configuration.
    #[error("failed to load config from {path}: {source}")]
    ConfigLoad {
        /// Path to the configuration file.
        path: PathBuf,
        /// Underlying IO or deserialization error.
        #[source]
        source: anyhow::Error,
    },

    /// No GBA context (project root) was found.
    #[error("no project root found: walked up from {start_dir} without finding .git/ or .gba/")]
    ContextNotFound {
        /// Directory where the search started.
        start_dir: PathBuf,
    },

    /// The requested feature was not found.
    #[error("feature not found: {slug}")]
    FeatureNotFound {
        /// Slug of the feature that was not found.
        slug: String,
    },

    /// The expected git worktree for a feature was not found.
    #[error("worktree not found at {path}")]
    WorktreeNotFound {
        /// Expected worktree path.
        path: PathBuf,
    },

    /// The project has not been initialized with `gba init`.
    #[error("project not initialized — run `gba init` first")]
    NotInitialized,

    /// A task with the given ID was not found in the feature state.
    #[error("task {task_id} not found in feature state")]
    TaskNotFound {
        /// ID of the task that was not found.
        task_id: u32,
    },
}
