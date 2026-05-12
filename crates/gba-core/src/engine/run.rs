//! Run engine — executes the implementation plan for a feature.
//!
//! The `RunEngine` drives the `gba run` workflow:
//! 1. Discover the feature directory and load state.
//! 2. Find the resume point (first non-completed task).
//! 3. Execute each task in order via the Claude Agent SDK.
//! 4. Update state after each task and save to disk.
//! 5. Return aggregate results including PR URL if created.

use std::path::{Path, PathBuf};

use gba_pm::PromptManager;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::{
    context::GbaContext,
    error::GbaCoreError,
    preset::PresetKind,
    runner::{AgentRunner, render_prompt},
    state::FeatureState,
    types::{FeatureStatus, RunResult, TaskKind, TaskProgress, TaskStatus, validate_slug},
};

/// Orchestrates the `gba run` workflow.
///
/// Loads the feature state, finds the resume point, and executes each
/// remaining task sequentially via the Claude Agent SDK.
///
/// # Usage
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use gba_core::{GbaContext, engine::run::RunEngine};
///
/// let ctx = GbaContext::load(&std::env::current_dir()?).await?;
/// let engine = RunEngine::new(ctx, "my-feature".to_owned())?;
/// let result = engine.run(|progress| {
///     println!("[{}] {}", progress.kind_label(), progress.description);
/// }).await?;
/// # Ok(())
/// # }
/// ```
pub struct RunEngine {
    ctx: GbaContext,
    slug: String,
    runner: AgentRunner,
    pm: PromptManager,
}

impl std::fmt::Debug for RunEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunEngine")
            .field("slug", &self.slug)
            .finish()
    }
}

impl RunEngine {
    /// Create a new `RunEngine` for the given feature slug.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::InvalidSlug` if the slug is not valid.
    /// Returns `GbaCoreError::PromptRender` if the prompt manager fails to initialize.
    pub fn new(ctx: GbaContext, slug: String) -> Result<Self, GbaCoreError> {
        validate_slug(&slug)?;
        let runner = AgentRunner::new(
            ctx.project_root.clone(),
            ctx.config.model.clone(),
            ctx.config.max_budget_usd,
        );
        let pm = PromptManager::new().map_err(|e| GbaCoreError::PromptRender {
            template: "<run-init>".to_owned(),
            source: e,
        })?;

        Ok(Self {
            ctx,
            slug,
            runner,
            pm,
        })
    }

    /// Execute all remaining tasks for the feature.
    ///
    /// Resumes from the first non-completed task. Calls `on_progress` after
    /// each task completes (or fails) so the caller can display status.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError` on filesystem, prompt rendering, or agent errors.
    pub async fn run(&self, on_progress: impl Fn(TaskProgress)) -> Result<RunResult, GbaCoreError> {
        // Step 1: Find feature directory
        let feature_dir = find_feature_dir(&self.ctx.gba_dir, &self.slug).await?;
        let state_path = feature_dir.join("state.yml");

        // Step 2: Load state
        let mut state = FeatureState::load(&state_path).await?;

        // Step 3: Load spec files (design and impl-plan are required; verification is optional)
        let specs_dir = feature_dir.join("specs");
        let design_spec = read_required_spec_file(&specs_dir, "design.md").await?;
        let verification_spec = read_optional_spec_file(&specs_dir, "verification.md").await;
        let impl_plan = read_required_spec_file(&specs_dir, "impl-plan.md").await?;

        // Step 4: Validate worktree
        let worktree_name = format!("{:04}_{}", state.feature.number, self.slug);
        let worktree_path = self.ctx.trees_dir.join(&worktree_name);
        if !tokio::fs::try_exists(&worktree_path).await.unwrap_or(false) {
            return Err(GbaCoreError::WorktreeNotFound {
                path: worktree_path,
            });
        }

        // Step 5: Find resume point
        let resume_idx = match state.find_resume_point() {
            Some(idx) => idx,
            None => {
                info!(slug = %self.slug, "all tasks already completed");
                return Ok(RunResult {
                    pr_url: state.pr.url.clone(),
                    total_turns: state.totals.turns,
                    total_cost_usd: state.totals.cost_usd,
                });
            }
        };

        // Step 6: Set feature status to Running
        state.status = FeatureStatus::Running;
        state.save().await?;

        // Step 7: Render system prompt
        let system_prompt = render_prompt(
            &self.pm,
            "run-system",
            &json!({ "feature_slug": self.slug }),
        )?;

        // Step 8: Execute tasks from resume point
        let mut review_findings = String::new();

        let task_count = state.tasks.len();
        for idx in resume_idx..task_count {
            let task_id = state.tasks[idx].id;
            let task_kind = state.tasks[idx].kind.clone();
            let task_description = state.tasks[idx].description.clone();
            let task_status = state.tasks[idx].status.clone();

            info!(
                task_id,
                kind = %task_kind.label(),
                description = %task_description,
                "starting task"
            );

            let result = self
                .execute_task(
                    &mut state,
                    idx,
                    &system_prompt,
                    &worktree_path,
                    &design_spec,
                    &verification_spec,
                    &impl_plan,
                    &task_kind,
                    &task_description,
                    &task_status,
                    &review_findings,
                )
                .await;

            match result {
                Ok(task_output) => {
                    // Store review findings for ReviewFix
                    if task_kind == TaskKind::Review {
                        review_findings = task_output;
                    }

                    on_progress(TaskProgress {
                        task_id,
                        kind: task_kind,
                        description: task_description,
                        status: TaskStatus::Completed,
                    });
                }
                Err(e) => {
                    warn!(task_id, error = %e, "task failed");

                    // Mark the task as Failed
                    let _ = state.update_task(task_id, TaskStatus::Failed, 0, 0.0).await;
                    state.status = FeatureStatus::Failed;
                    state.save().await?;

                    on_progress(TaskProgress {
                        task_id,
                        kind: task_kind,
                        description: task_description,
                        status: TaskStatus::Failed,
                    });

                    return Err(e);
                }
            }
        }

        // Step 9: Mark feature as Completed
        state.status = FeatureStatus::Completed;
        state.save().await?;

        info!(
            slug = %self.slug,
            turns = state.totals.turns,
            cost_usd = state.totals.cost_usd,
            "run completed"
        );

        Ok(RunResult {
            pr_url: state.pr.url.clone(),
            total_turns: state.totals.turns,
            total_cost_usd: state.totals.cost_usd,
        })
    }

    /// Execute a single task based on its kind.
    ///
    /// Returns the agent's text response on success for use by downstream tasks
    /// (e.g., Review findings used by ReviewFix).
    #[allow(clippy::too_many_arguments)]
    async fn execute_task(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        design_spec: &str,
        verification_spec: &str,
        impl_plan: &str,
        task_kind: &TaskKind,
        task_description: &str,
        task_status: &TaskStatus,
        review_findings: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        match task_kind {
            TaskKind::Setup => {
                // Setup was already done during plan; mark completed immediately.
                state
                    .update_task(task_id, TaskStatus::Completed, 0, 0.0)
                    .await?;
                Ok(String::new())
            }
            TaskKind::Build => {
                self.execute_build(
                    state,
                    idx,
                    system_prompt,
                    worktree_path,
                    design_spec,
                    verification_spec,
                    impl_plan,
                    task_description,
                    task_status,
                )
                .await
            }
            TaskKind::Commit => {
                self.execute_commit(state, idx, system_prompt, worktree_path, task_description)
                    .await
            }
            TaskKind::Verification => {
                self.execute_verification(
                    state,
                    idx,
                    system_prompt,
                    worktree_path,
                    verification_spec,
                )
                .await
            }
            TaskKind::Review => {
                self.execute_review(
                    state,
                    idx,
                    system_prompt,
                    worktree_path,
                    design_spec,
                    verification_spec,
                )
                .await
            }
            TaskKind::ReviewFix => {
                self.execute_review_fix(
                    state,
                    idx,
                    system_prompt,
                    worktree_path,
                    design_spec,
                    review_findings,
                )
                .await
            }
            TaskKind::SubmitPr => {
                self.execute_submit_pr(state, idx, system_prompt, worktree_path, design_spec)
                    .await
            }
        }
    }

    /// Execute a Build task (or BuildResume if previously interrupted).
    #[allow(clippy::too_many_arguments)]
    async fn execute_build(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        design_spec: &str,
        verification_spec: &str,
        impl_plan: &str,
        task_description: &str,
        task_status: &TaskStatus,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        let is_resume = *task_status == TaskStatus::Running || *task_status == TaskStatus::Failed;
        let preset_kind = if is_resume {
            PresetKind::BuildResume
        } else {
            PresetKind::Build
        };

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        // Extract phase number from description (e.g., "Phase 1: ..." -> "1")
        let phase_number = extract_phase_number(task_description);

        let user_prompt = if is_resume {
            let previous_status = format!("{task_status:?}");
            render_prompt(
                &self.pm,
                "run-build-phase-resume",
                &json!({
                    "phase_number": phase_number,
                    "phase_description": task_description,
                    "previous_status": previous_status,
                    "design_spec": design_spec,
                    "impl_plan": impl_plan,
                    "verification_spec": verification_spec,
                }),
            )?
        } else {
            render_prompt(
                &self.pm,
                "run-build-phase",
                &json!({
                    "phase_number": phase_number,
                    "phase_description": task_description,
                    "design_spec": design_spec,
                    "impl_plan": impl_plan,
                    "verification_spec": verification_spec,
                }),
            )?
        };

        let preset = preset_kind.apply_config_override(self.max_turns_override_for("build"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        debug!(
            task_id,
            turns = result.turns,
            cost_usd = result.cost_usd,
            "build task completed"
        );

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Execute a Commit task.
    async fn execute_commit(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        task_description: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        let phase_number = extract_phase_number(task_description);
        // Strip "Commit: " prefix for the phase description if present
        let phase_description = task_description
            .strip_prefix("Commit: ")
            .unwrap_or(task_description);

        let user_prompt = render_prompt(
            &self.pm,
            "run-commit",
            &json!({
                "feature_slug": self.slug,
                "phase_number": phase_number,
                "phase_description": phase_description,
            }),
        )?;

        let preset =
            PresetKind::Commit.apply_config_override(self.max_turns_override_for("commit"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        // Try to extract commit SHA from response
        if let Some(sha) = extract_commit_sha(&result.text) {
            debug!(task_id, sha = %sha, "extracted commit SHA");
            if let Some(task) = state.tasks.iter_mut().find(|t| t.id == task_id) {
                task.commit_sha = Some(sha);
            }
        }

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Execute a Verification task.
    async fn execute_verification(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        verification_spec: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        let user_prompt = render_prompt(
            &self.pm,
            "run-verification",
            &json!({
                "feature_slug": self.slug,
                "verification_spec": verification_spec,
            }),
        )?;

        let preset = PresetKind::Verification
            .apply_config_override(self.max_turns_override_for("verification"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Execute a Review task (read-only, Plan mode).
    async fn execute_review(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        design_spec: &str,
        verification_spec: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        // Get the diff of all changes from main
        let all_changes_diff = get_git_diff(worktree_path).await;

        let user_prompt = render_prompt(
            &self.pm,
            "run-review",
            &json!({
                "feature_slug": self.slug,
                "design_spec": design_spec,
                "verification_spec": verification_spec,
                "all_changes_diff": all_changes_diff,
            }),
        )?;

        let preset =
            PresetKind::Review.apply_config_override(self.max_turns_override_for("review"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Execute a ReviewFix task.
    async fn execute_review_fix(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        design_spec: &str,
        review_findings: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        let user_prompt = render_prompt(
            &self.pm,
            "run-review-fix",
            &json!({
                "feature_slug": self.slug,
                "review_findings": review_findings,
                "design_spec": design_spec,
            }),
        )?;

        let preset =
            PresetKind::ReviewFix.apply_config_override(self.max_turns_override_for("review_fix"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Execute a SubmitPr task.
    async fn execute_submit_pr(
        &self,
        state: &mut FeatureState,
        idx: usize,
        system_prompt: &str,
        worktree_path: &Path,
        design_spec: &str,
    ) -> Result<String, GbaCoreError> {
        let task_id = state.tasks[idx].id;

        // Mark as running
        state.tasks[idx].status = TaskStatus::Running;
        state.save().await?;

        let user_prompt = render_prompt(
            &self.pm,
            "run-submit-pr",
            &json!({
                "feature_slug": self.slug,
                "design_spec": design_spec,
            }),
        )?;

        let preset =
            PresetKind::SubmitPr.apply_config_override(self.max_turns_override_for("submit_pr"));
        let wt = worktree_path.to_path_buf();
        let result = self
            .runner
            .query(&preset, system_prompt, &user_prompt, Some(&wt))
            .await?;

        // Try to extract PR URL from response
        if let Some(pr_url) = extract_pr_url(&result.text) {
            info!(pr_url = %pr_url, "extracted PR URL");
            state.pr.url = pr_url;
            // Try to extract PR number from the URL
            state.pr.number = extract_pr_number(&state.pr.url);
        }

        state
            .update_task(
                task_id,
                TaskStatus::Completed,
                result.turns,
                result.cost_usd,
            )
            .await?;

        Ok(result.text)
    }

    /// Look up the `max_turns` override for a given preset name from config.
    fn max_turns_override_for(&self, preset_name: &str) -> Option<u32> {
        self.ctx
            .config
            .presets
            .get(preset_name)
            .and_then(|p| p.max_turns)
    }
}

/// Scan `.gba/` for a directory matching `NNNN_<slug>`.
///
/// # Errors
///
/// Returns `GbaCoreError::FeatureNotFound` if no matching directory is found.
/// Returns `GbaCoreError::StateLoad` if the directory cannot be read.
async fn find_feature_dir(gba_dir: &Path, slug: &str) -> Result<PathBuf, GbaCoreError> {
    let suffix = format!("_{slug}");

    let mut entries = tokio::fs::read_dir(gba_dir)
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: gba_dir.to_owned(),
            source: e.into(),
        })?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: gba_dir.to_owned(),
            source: e.into(),
        })?
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.ends_with(&suffix)
            && let Some(prefix) = name_str.strip_suffix(&suffix)
            && !prefix.is_empty()
            && prefix.chars().all(|c| c.is_ascii_digit())
        {
            let path = entry.path();
            if let Ok(meta) = tokio::fs::metadata(&path).await
                && meta.is_dir()
            {
                return Ok(path);
            }
        }
    }

    Err(GbaCoreError::FeatureNotFound {
        slug: slug.to_owned(),
    })
}

/// Read a required spec file from the specs directory.
///
/// # Errors
///
/// Returns `GbaCoreError::StateLoad` if the file cannot be read.
async fn read_required_spec_file(specs_dir: &Path, filename: &str) -> Result<String, GbaCoreError> {
    let path = specs_dir.join(filename);
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path,
            source: e.into(),
        })
}

/// Read an optional spec file from the specs directory.
///
/// Returns an empty string if the file does not exist, logging a warning.
async fn read_optional_spec_file(specs_dir: &Path, filename: &str) -> String {
    let path = specs_dir.join(filename);
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "optional spec file not found, continuing with empty content");
            String::new()
        }
    }
}

/// Extract a phase number from a task description.
///
/// Looks for patterns like "Phase 1: ..." or "Phase 2 - ..." in the text.
/// Returns the phase number as a string, or "1" as a fallback.
fn extract_phase_number(description: &str) -> String {
    let lower = description.to_lowercase();

    // Look for "phase N" pattern
    if let Some(pos) = lower.find("phase") {
        let after = &description[pos.saturating_add("phase".len())..];
        let trimmed = after.trim_start();
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
    }

    // Fallback
    "1".to_owned()
}

/// Extract a commit SHA from agent response text.
///
/// Looks for patterns like:
/// - `[abc1234]` (square-bracket wrapped short SHA)
/// - `commit abc1234` (git log format)
/// - 7-40 character hex strings following common git output patterns
///
/// Returns the first match found, or `None`.
fn extract_commit_sha(text: &str) -> Option<String> {
    // Pattern 1: "[<hex>]" — 7-40 hex chars in square brackets
    for line in text.lines() {
        let trimmed = line.trim();

        // Look for [<sha>] pattern
        if let Some(start) = trimmed.find('[')
            && let Some(end) = trimmed[start..].find(']')
        {
            let candidate = &trimmed[start.saturating_add(1)..start.saturating_add(end)];
            if is_hex_sha(candidate) {
                return Some(candidate.to_owned());
            }
        }
    }

    // Pattern 2: "commit <hex>" pattern (git log output)
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("commit ") {
            let candidate: &str = rest.split_whitespace().next().unwrap_or("");
            if is_hex_sha(candidate) {
                return Some(candidate.to_owned());
            }
        }
    }

    None
}

/// Check if a string is a valid short or full git SHA (7-40 hex characters).
fn is_hex_sha(s: &str) -> bool {
    let len = s.len();
    (7..=40).contains(&len) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Extract a GitHub PR URL from agent response text.
///
/// Looks for `https://github.com/` URLs containing `/pull/` in the path.
fn extract_pr_url(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        // Strip common trailing punctuation
        let cleaned = word.trim_end_matches(['.', ',', ')', ']', '>', '"', '\'']);

        if cleaned.starts_with("https://github.com/") && cleaned.contains("/pull/") {
            return Some(cleaned.to_owned());
        }
    }
    None
}

/// Extract PR number from a GitHub PR URL.
///
/// Expects URLs like `https://github.com/owner/repo/pull/123`.
fn extract_pr_number(url: &str) -> u32 {
    url.rsplit('/')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Get the git diff of all changes from the main branch.
///
/// Runs `git diff main...HEAD` in the worktree directory.
/// Falls back to `git diff master...HEAD` if the first command fails.
/// Returns an empty string if both fail.
async fn get_git_diff(worktree_path: &Path) -> String {
    // Try main first
    let output = tokio::process::Command::new("git")
        .args(["diff", "main...HEAD"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(output) = output
        && output.status.success()
    {
        return String::from_utf8_lossy(&output.stdout).to_string();
    }

    // Fallback to master
    let output = tokio::process::Command::new("git")
        .args(["diff", "master...HEAD"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(output) = output
        && output.status.success()
    {
        return String::from_utf8_lossy(&output.stdout).to_string();
    }

    warn!("failed to get git diff — using empty diff");
    String::new()
}

impl TaskKind {
    /// Return a human-readable label for display.
    ///
    /// # Examples
    ///
    /// ```
    /// use gba_core::types::TaskKind;
    /// assert_eq!(TaskKind::Build.label(), "Build");
    /// assert_eq!(TaskKind::SubmitPr.label(), "Submit PR");
    /// ```
    pub fn label(&self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Build => "Build",
            Self::Commit => "Commit",
            Self::Verification => "Verification",
            Self::Review => "Review",
            Self::ReviewFix => "Review Fix",
            Self::SubmitPr => "Submit PR",
        }
    }
}

impl TaskProgress {
    /// Return the label for the task kind.
    pub fn kind_label(&self) -> &'static str {
        self.kind.label()
    }
}

impl std::fmt::Display for TaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    // ---------- find_feature_dir ----------

    #[tokio::test]
    async fn test_should_find_feature_dir_by_slug() {
        let dir = TempDir::new().expect("test: create temp dir");
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.expect("test: mkdir");

        // Create matching directory
        tokio::fs::create_dir(gba_dir.join("0001_my-feature"))
            .await
            .expect("test: mkdir");

        // Create non-matching items
        tokio::fs::create_dir(gba_dir.join("0002_other-feature"))
            .await
            .expect("test: mkdir");
        tokio::fs::write(gba_dir.join("config.yml"), "model: test")
            .await
            .expect("test: write");

        let result = find_feature_dir(&gba_dir, "my-feature").await;
        assert!(result.is_ok());
        let path = result.expect("should find feature dir");
        assert!(path.ends_with("0001_my-feature"));
    }

    #[tokio::test]
    async fn test_should_return_error_when_feature_not_found() {
        let dir = TempDir::new().expect("test: create temp dir");
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.expect("test: mkdir");

        let result = find_feature_dir(&gba_dir, "nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaCoreError::FeatureNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_should_ignore_files_matching_pattern() {
        let dir = TempDir::new().expect("test: create temp dir");
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.expect("test: mkdir");

        // Create a file (not directory) with matching name
        tokio::fs::write(gba_dir.join("0001_file-feature"), "not a dir")
            .await
            .expect("test: write");

        let result = find_feature_dir(&gba_dir, "file-feature").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_should_ignore_dirs_without_numeric_prefix() {
        let dir = TempDir::new().expect("test: create temp dir");
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.expect("test: mkdir");

        // Create directory without numeric prefix
        tokio::fs::create_dir(gba_dir.join("abc_my-feature"))
            .await
            .expect("test: mkdir");

        let result = find_feature_dir(&gba_dir, "my-feature").await;
        assert!(result.is_err());
    }

    // ---------- extract_commit_sha ----------

    #[test]
    fn test_should_extract_sha_from_bracket_pattern() {
        let text = "Created commit [abc1234def] for phase 1.";
        assert_eq!(extract_commit_sha(text), Some("abc1234def".to_owned()));
    }

    #[test]
    fn test_should_extract_sha_from_git_commit_line() {
        let text = "commit 1a2b3c4d5e6f7890abcdef1234567890abcdef12\nAuthor: Test";
        assert_eq!(
            extract_commit_sha(text),
            Some("1a2b3c4d5e6f7890abcdef1234567890abcdef12".to_owned())
        );
    }

    #[test]
    fn test_should_return_none_for_no_sha() {
        let text = "No commit information here.";
        assert_eq!(extract_commit_sha(text), None);
    }

    #[test]
    fn test_should_reject_too_short_sha() {
        let text = "[abc12]";
        assert_eq!(extract_commit_sha(text), None);
    }

    #[test]
    fn test_should_reject_non_hex_in_brackets() {
        let text = "[not_a_sha_value]";
        assert_eq!(extract_commit_sha(text), None);
    }

    // ---------- extract_pr_url ----------

    #[test]
    fn test_should_extract_pr_url() {
        let text = "PR created: https://github.com/owner/repo/pull/42";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/owner/repo/pull/42".to_owned())
        );
    }

    #[test]
    fn test_should_extract_pr_url_with_trailing_punctuation() {
        let text = "See https://github.com/owner/repo/pull/42.";
        assert_eq!(
            extract_pr_url(text),
            Some("https://github.com/owner/repo/pull/42".to_owned())
        );
    }

    #[test]
    fn test_should_return_none_for_no_pr_url() {
        let text = "No PR was created.";
        assert_eq!(extract_pr_url(text), None);
    }

    #[test]
    fn test_should_reject_non_github_urls() {
        let text = "https://gitlab.com/owner/repo/pull/42";
        assert_eq!(extract_pr_url(text), None);
    }

    #[test]
    fn test_should_reject_github_url_without_pull() {
        let text = "https://github.com/owner/repo/issues/42";
        assert_eq!(extract_pr_url(text), None);
    }

    // ---------- extract_pr_number ----------

    #[test]
    fn test_should_extract_pr_number_from_url() {
        assert_eq!(
            extract_pr_number("https://github.com/owner/repo/pull/123"),
            123
        );
    }

    #[test]
    fn test_should_return_zero_for_invalid_pr_number() {
        assert_eq!(
            extract_pr_number("https://github.com/owner/repo/pull/abc"),
            0
        );
    }

    // ---------- extract_phase_number ----------

    #[test]
    fn test_should_extract_phase_number_from_description() {
        assert_eq!(extract_phase_number("Phase 1: Foundation"), "1");
        assert_eq!(extract_phase_number("Phase 2 - Integration"), "2");
        assert_eq!(extract_phase_number("Phase 10: Large number"), "10");
    }

    #[test]
    fn test_should_return_fallback_for_no_phase() {
        assert_eq!(extract_phase_number("Set up scaffolding"), "1");
    }

    #[test]
    fn test_should_handle_commit_description_with_phase() {
        assert_eq!(extract_phase_number("Commit: Phase 3: Polish"), "3");
    }

    // ---------- is_hex_sha ----------

    #[test]
    fn test_should_validate_hex_sha() {
        assert!(is_hex_sha("abc1234"));
        assert!(is_hex_sha("1a2b3c4d5e6f7890abcdef1234567890abcdef12"));
        assert!(!is_hex_sha("abc12")); // too short
        assert!(!is_hex_sha("xyz1234")); // non-hex
        assert!(!is_hex_sha("")); // empty
    }

    // ---------- TaskKind::label ----------

    #[test]
    fn test_should_return_label_for_all_task_kinds() {
        assert_eq!(TaskKind::Setup.label(), "Setup");
        assert_eq!(TaskKind::Build.label(), "Build");
        assert_eq!(TaskKind::Commit.label(), "Commit");
        assert_eq!(TaskKind::Verification.label(), "Verification");
        assert_eq!(TaskKind::Review.label(), "Review");
        assert_eq!(TaskKind::ReviewFix.label(), "Review Fix");
        assert_eq!(TaskKind::SubmitPr.label(), "Submit PR");
    }

    // ---------- TaskKind Display ----------

    #[test]
    fn test_should_display_task_kind() {
        assert_eq!(format!("{}", TaskKind::Build), "Build");
        assert_eq!(format!("{}", TaskKind::SubmitPr), "Submit PR");
    }

    // ---------- resume point with feature dir ----------

    #[tokio::test]
    async fn test_should_find_feature_dir_with_higher_numbers() {
        let dir = TempDir::new().expect("test: create temp dir");
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.expect("test: mkdir");

        tokio::fs::create_dir(gba_dir.join("0042_my-feature"))
            .await
            .expect("test: mkdir");

        let result = find_feature_dir(&gba_dir, "my-feature").await;
        assert!(result.is_ok());
        let path = result.expect("should find feature dir");
        assert!(path.ends_with("0042_my-feature"));
    }
}
