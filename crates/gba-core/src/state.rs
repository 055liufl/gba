//! Feature state persistence — load, save, and update `state.yml`.
//!
//! Each feature has a `state.yml` file at `.gba/<NNNN>_<slug>/state.yml`
//! that tracks the full lifecycle. This file enables resume on interruption
//! and cost tracking.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    error::GbaCoreError,
    types::{FeatureStatus, TaskKind, TaskStatus},
};

/// Persistent state for a single task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    /// Task ID (1-based, unique within the feature).
    pub id: u32,
    /// Kind of task.
    pub kind: TaskKind,
    /// Human-readable description.
    pub description: String,
    /// Current execution status.
    pub status: TaskStatus,
    /// Number of turns consumed so far.
    #[serde(default)]
    pub turns: u32,
    /// Cost in USD consumed so far.
    #[serde(default)]
    pub cost_usd: f64,
    /// Git commit SHA produced by this task (only for `Commit` tasks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Timestamp when the task completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Persistent state for the planning phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanState {
    /// Turns consumed during planning.
    pub turns: u32,
    /// Cost in USD consumed during planning.
    pub cost_usd: f64,
    /// Timestamp when planning completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Pull request metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrState {
    /// PR URL (empty until PR is created).
    #[serde(default)]
    pub url: String,
    /// PR number (0 until PR is created).
    #[serde(default)]
    pub number: u32,
}

/// Aggregate totals for turns and cost.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TotalsState {
    /// Total turns across all tasks.
    #[serde(default)]
    pub turns: u32,
    /// Total cost in USD across all tasks.
    #[serde(default)]
    pub cost_usd: f64,
}

/// Identifying information for a feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureInfo {
    /// Sequential feature number.
    pub number: u32,
    /// URL-friendly slug.
    pub slug: String,
    /// Git branch name for this feature.
    pub branch: String,
    /// Timestamp when the feature was created.
    pub created_at: DateTime<Utc>,
}

/// Top-level feature state persisted as `state.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureState {
    /// Feature identifying information.
    pub feature: FeatureInfo,
    /// High-level feature status.
    pub status: FeatureStatus,
    /// Planning phase state (populated after planning completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanState>,
    /// Ordered list of tasks in the implementation plan.
    #[serde(default)]
    pub tasks: Vec<TaskState>,
    /// Aggregate totals.
    #[serde(default)]
    pub totals: TotalsState,
    /// Pull request metadata.
    #[serde(default)]
    pub pr: PrState,

    /// Path to the state file on disk (not serialized).
    #[serde(skip)]
    path: PathBuf,
}

impl FeatureState {
    /// Load feature state from a YAML file.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::StateLoad` if the file cannot be read or parsed.
    pub async fn load(path: &Path) -> Result<Self, GbaCoreError> {
        let contents =
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| GbaCoreError::StateLoad {
                    path: path.to_owned(),
                    source: e.into(),
                })?;

        let mut state: Self =
            serde_yml::from_str(&contents).map_err(|e| GbaCoreError::StateLoad {
                path: path.to_owned(),
                source: e.into(),
            })?;

        state.path = path.to_owned();
        Ok(state)
    }

    /// Save feature state to disk atomically (write to `.tmp` then rename).
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::StateSave` if serialization or file IO fails.
    pub async fn save(&self) -> Result<(), GbaCoreError> {
        let yaml = serde_yml::to_string(self).map_err(|e| GbaCoreError::StateSave {
            path: self.path.clone(),
            source: e.into(),
        })?;

        let tmp_path = self.path.with_extension("yml.tmp");

        tokio::fs::write(&tmp_path, yaml.as_bytes())
            .await
            .map_err(|e| GbaCoreError::StateSave {
                path: self.path.clone(),
                source: e.into(),
            })?;

        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|e| GbaCoreError::StateSave {
                path: self.path.clone(),
                source: e.into(),
            })?;

        Ok(())
    }

    /// Return the path to the state file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set the path for this state (used when creating a new state).
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    /// Find the index of the first task whose status is neither `Completed`
    /// nor `Skipped`. Returns `None` if all tasks are done.
    pub fn find_resume_point(&self) -> Option<usize> {
        self.tasks
            .iter()
            .position(|t| t.status != TaskStatus::Completed && t.status != TaskStatus::Skipped)
    }

    /// Update a task by ID, set its status, accumulate turns and cost, then save.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::TaskNotFound` if no task has the given ID.
    /// Returns `GbaCoreError::StateSave` if saving fails.
    pub async fn update_task(
        &mut self,
        task_id: u32,
        status: TaskStatus,
        turns: u32,
        cost_usd: f64,
    ) -> Result<(), GbaCoreError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or(GbaCoreError::TaskNotFound { task_id })?;

        task.status = status.clone();
        task.turns = task.turns.saturating_add(turns);
        task.cost_usd += cost_usd;

        if status == TaskStatus::Completed {
            task.completed_at = Some(Utc::now());
        }

        // Recompute totals from all tasks.
        self.totals.turns = self.tasks.iter().map(|t| t.turns).sum();
        self.totals.cost_usd = self.tasks.iter().map(|t| t.cost_usd).sum();

        self.save().await
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::TempDir;

    use super::*;

    fn sample_state(path: PathBuf) -> FeatureState {
        FeatureState {
            feature: FeatureInfo {
                number: 1,
                slug: "test-feature".to_owned(),
                branch: "feat/0001-test-feature".to_owned(),
                created_at: Utc::now(),
            },
            status: FeatureStatus::Running,
            plan: Some(PlanState {
                turns: 5,
                cost_usd: 0.25,
                completed_at: Some(Utc::now()),
            }),
            tasks: vec![
                TaskState {
                    id: 1,
                    kind: TaskKind::Setup,
                    description: "Generate directory structure".to_owned(),
                    status: TaskStatus::Completed,
                    turns: 0,
                    cost_usd: 0.0,
                    commit_sha: None,
                    completed_at: Some(Utc::now()),
                },
                TaskState {
                    id: 2,
                    kind: TaskKind::Build,
                    description: "Phase 1: Build module".to_owned(),
                    status: TaskStatus::Running,
                    turns: 5,
                    cost_usd: 0.50,
                    commit_sha: None,
                    completed_at: None,
                },
                TaskState {
                    id: 3,
                    kind: TaskKind::Commit,
                    description: "Commit phase 1".to_owned(),
                    status: TaskStatus::Pending,
                    turns: 0,
                    cost_usd: 0.0,
                    commit_sha: None,
                    completed_at: None,
                },
            ],
            totals: TotalsState {
                turns: 5,
                cost_usd: 0.50,
            },
            pr: PrState::default(),
            path,
        }
    }

    #[tokio::test]
    async fn test_should_roundtrip_state_through_yaml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.yml");

        let state = sample_state(path.clone());
        state.save().await.unwrap();

        let loaded = FeatureState::load(&path).await.unwrap();
        assert_eq!(loaded.feature.number, 1);
        assert_eq!(loaded.feature.slug, "test-feature");
        assert_eq!(loaded.status, FeatureStatus::Running);
        assert_eq!(loaded.tasks.len(), 3);
        assert_eq!(loaded.tasks[0].status, TaskStatus::Completed);
        assert_eq!(loaded.tasks[1].status, TaskStatus::Running);
        assert_eq!(loaded.tasks[2].status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_should_find_resume_point_at_running_task() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.yml");
        let state = sample_state(path);

        // Task 0 is Completed, task 1 is Running => resume at index 1
        assert_eq!(state.find_resume_point(), Some(1));
    }

    #[tokio::test]
    async fn test_should_return_none_when_all_completed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.yml");
        let mut state = sample_state(path);
        for task in &mut state.tasks {
            task.status = TaskStatus::Completed;
        }
        assert_eq!(state.find_resume_point(), None);
    }

    #[tokio::test]
    async fn test_should_update_task_and_recompute_totals() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.yml");
        let mut state = sample_state(path);
        state.save().await.unwrap();

        state
            .update_task(2, TaskStatus::Completed, 3, 0.30)
            .await
            .unwrap();

        assert_eq!(state.tasks[1].status, TaskStatus::Completed);
        assert_eq!(state.tasks[1].turns, 8); // 5 + 3
        assert!((state.tasks[1].cost_usd - 0.80).abs() < 0.001); // 0.50 + 0.30
        assert!(state.tasks[1].completed_at.is_some());

        // Totals recomputed: task 1 (0) + task 2 (8) + task 3 (0) = 8
        assert_eq!(state.totals.turns, 8);
    }

    #[tokio::test]
    async fn test_should_return_error_for_unknown_task_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.yml");
        let mut state = sample_state(path);
        state.save().await.unwrap();

        let result = state.update_task(999, TaskStatus::Completed, 0, 0.0).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaCoreError::TaskNotFound { task_id: 999 }
        ));
    }

    #[tokio::test]
    async fn test_should_return_error_loading_nonexistent_file() {
        let result = FeatureState::load(Path::new("/tmp/nonexistent/state.yml")).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaCoreError::StateLoad { .. }
        ));
    }
}
