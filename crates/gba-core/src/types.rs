//! Shared domain types for the GBA engine.
//!
//! This module contains enums, structs, and result types used across
//! the core engine, CLI, and state persistence layers.

use std::{path::PathBuf, sync::LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::GbaCoreError;

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
    /// Whether this was a first-time initialization (`true`) or an incremental
    /// update of an already-initialized project (`false`).
    pub is_fresh_init: bool,
    /// Summary text from the repo analysis (empty for incremental runs).
    pub summary: String,
    /// Number of context documents generated or regenerated.
    pub context_doc_count: usize,
    /// Number of context documents skipped because their directory contents
    /// were unchanged (incremental update only).
    pub context_docs_skipped: usize,
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

/// Maximum byte length of a slug.
const MAX_SLUG_LENGTH: usize = 64;

/// Compiled regex for slug validation: one or more groups of lowercase
/// alphanumeric characters separated by single hyphens.
static SLUG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("slug regex is valid"));

/// Validate that a slug is safe for use in file paths and git branch names.
///
/// A valid slug must:
/// - Be at most 64 bytes long
/// - Match `^[a-z0-9]+(?:-[a-z0-9]+)*$` (lowercase alphanumeric segments separated by single
///   hyphens)
///
/// # Errors
///
/// Returns `GbaCoreError::InvalidSlug` if the slug does not match the pattern
/// or exceeds the length limit.
///
/// # Examples
///
/// ```
/// use gba_core::types::validate_slug;
///
/// assert!(validate_slug("web-frontend").is_ok());
/// assert!(validate_slug("my-feature-123").is_ok());
/// assert!(validate_slug("../../etc").is_err());
/// assert!(validate_slug("").is_err());
/// ```
pub fn validate_slug(slug: &str) -> Result<(), GbaCoreError> {
    if slug.len() > MAX_SLUG_LENGTH || !SLUG_REGEX.is_match(slug) {
        return Err(GbaCoreError::InvalidSlug {
            slug: slug.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_accept_valid_slug() {
        assert!(validate_slug("web-frontend").is_ok());
        assert!(validate_slug("my-feature").is_ok());
        assert!(validate_slug("feature123").is_ok());
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug("abc-def-ghi").is_ok());
        assert!(validate_slug("my-feature-123").is_ok());
    }

    #[test]
    fn test_should_reject_empty_slug() {
        assert!(validate_slug("").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_path_traversal() {
        assert!(validate_slug("../../etc").is_err());
        assert!(validate_slug("../x").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_absolute_path() {
        assert!(validate_slug("/etc").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_uppercase() {
        assert!(validate_slug("MyFeature").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_underscores() {
        assert!(validate_slug("my_feature").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_special_chars() {
        assert!(validate_slug("my feature").is_err());
        assert!(validate_slug("my.feature").is_err());
        assert!(validate_slug("my@feature").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_leading_hyphen() {
        assert!(validate_slug("-leading").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_trailing_hyphen() {
        assert!(validate_slug("trailing-").is_err());
    }

    #[test]
    fn test_should_reject_slug_with_consecutive_hyphens() {
        assert!(validate_slug("double--hyphen").is_err());
    }

    #[test]
    fn test_should_reject_slug_exceeding_max_length() {
        let long_slug = "a".repeat(MAX_SLUG_LENGTH + 1);
        assert!(validate_slug(&long_slug).is_err());
    }

    #[test]
    fn test_should_accept_slug_at_max_length() {
        let slug = "a".repeat(MAX_SLUG_LENGTH);
        assert!(validate_slug(&slug).is_ok());
    }
}
