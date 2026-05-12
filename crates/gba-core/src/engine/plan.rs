//! Plan engine — multi-turn planning conversation, spec generation, and task list creation.
//!
//! The `PlanEngine` drives the `gba plan` workflow:
//! 1. Start a multi-turn conversation with the agent about the feature.
//! 2. Allow the user to iterate on the design via `send()`.
//! 3. Finalize: generate spec documents, create the feature directory and worktree, parse the
//!    implementation plan into a task list, and save `state.yml`.

use std::path::Path;

use chrono::Utc;
use claude_agent_sdk_rs::ClaudeClient;
use gba_pm::PromptManager;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::{
    context::GbaContext,
    error::GbaCoreError,
    preset::PresetKind,
    runner::{AgentRunner, render_prompt},
    state::{FeatureInfo, FeatureState, PlanState, TaskState, TotalsState},
    types::{FeatureStatus, PlanResult, TaskKind, TaskStatus, validate_slug},
};

/// Separator prefix used to delimit spec files in the agent's response.
const FILE_SEPARATOR_PREFIX: &str = "---FILE:";

/// Separator suffix used to delimit spec files in the agent's response.
const FILE_SEPARATOR_SUFFIX: &str = "---";

/// Orchestrates the `gba plan` workflow.
///
/// Manages a multi-turn conversation session with the agent, generates
/// specification documents, creates the feature directory and git worktree,
/// and persists the initial `state.yml`.
///
/// # Usage
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use gba_core::{GbaContext, engine::plan::PlanEngine};
///
/// let ctx = GbaContext::load(&std::env::current_dir()?).await?;
/// let mut engine = PlanEngine::new(ctx, "my-feature".to_owned())?;
/// let response = engine.start().await?;
/// // ... interact with engine.send() ...
/// let result = engine.finalize().await?;
/// # Ok(())
/// # }
/// ```
pub struct PlanEngine {
    ctx: GbaContext,
    slug: String,
    runner: AgentRunner,
    pm: PromptManager,
    client: Option<ClaudeClient>,
    turns: u32,
    cost_usd: f64,
}

impl std::fmt::Debug for PlanEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanEngine")
            .field("slug", &self.slug)
            .field("turns", &self.turns)
            .field("cost_usd", &self.cost_usd)
            .field("has_client", &self.client.is_some())
            .finish()
    }
}

impl PlanEngine {
    /// Create a new `PlanEngine` for the given feature slug.
    ///
    /// Initializes the agent runner and prompt manager but does not start
    /// the conversation session. Call [`start`](Self::start) to begin.
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
            template: "<plan-init>".to_owned(),
            source: e,
        })?;

        Ok(Self {
            ctx,
            slug,
            runner,
            pm,
            client: None,
            turns: 0,
            cost_usd: 0.0,
        })
    }

    /// Start the planning conversation session.
    ///
    /// Reads the project summary from `.gba/` analysis results (or uses a
    /// fallback), renders the system and initial user prompts, starts a
    /// multi-turn session, and sends the opening message.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError` if prompt rendering, session creation, or
    /// message sending fails.
    pub async fn start(&mut self) -> Result<String, GbaCoreError> {
        let project_summary = read_project_summary(&self.ctx).await;

        let system_prompt = render_prompt(
            &self.pm,
            "plan-system",
            &json!({ "project_summary": project_summary }),
        )?;

        let preset = PresetKind::PlanConversation.preset();
        let mut client = self
            .runner
            .start_session(&preset, &system_prompt, None)
            .await?;

        let user_prompt = render_prompt(
            &self.pm,
            "plan-start-conversation",
            &json!({ "feature_slug": self.slug }),
        )?;

        info!(slug = %self.slug, "starting planning conversation");
        let result = AgentRunner::send(&mut client, &user_prompt).await?;
        self.turns = self.turns.saturating_add(result.turns);
        self.cost_usd += result.cost_usd;
        self.client = Some(client);

        Ok(result.text)
    }

    /// Send a user message to the ongoing planning session.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::AgentQuery` if the session is not started or
    /// sending fails.
    pub async fn send(&mut self, user_input: &str) -> Result<String, GbaCoreError> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| GbaCoreError::AgentQuery {
                message: "planning session not started — call start() first".to_owned(),
                source: None,
            })?;

        let result = AgentRunner::send(client, user_input).await?;
        self.turns = self.turns.saturating_add(result.turns);
        self.cost_usd += result.cost_usd;

        Ok(result.text)
    }

    /// Finalize the plan: generate specs, create the feature directory and
    /// worktree, parse tasks, and save `state.yml`.
    ///
    /// This method consumes the active session. After finalization the engine
    /// cannot send further messages.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError` on prompt rendering, agent communication,
    /// filesystem, or git worktree errors.
    pub async fn finalize(&mut self) -> Result<PlanResult, GbaCoreError> {
        // Step 1: Generate spec documents
        let spec_prompt = render_prompt(
            &self.pm,
            "plan-generate-spec",
            &json!({ "feature_slug": self.slug }),
        )?;

        info!("generating specification documents");
        let spec_response = self.send_to_client(&spec_prompt).await?;
        let spec_files = parse_spec_files(&spec_response);

        // Step 2: Get next feature number and create directories
        let feature_number = self.ctx.next_feature_number().await?;
        let feature_dir = self.ctx.feature_dir_numbered(feature_number, &self.slug);
        let specs_dir = feature_dir.join("specs");

        create_dir(&feature_dir).await?;
        create_dir(&specs_dir).await?;
        info!(
            number = feature_number,
            path = %feature_dir.display(),
            "created feature directory"
        );

        // Step 3: Write spec files
        let mut design_spec = String::new();
        let mut verification_spec = String::new();
        let mut has_impl_plan = false;

        for (filename, content) in &spec_files {
            let file_path = specs_dir.join(filename);
            write_file(&file_path, content).await?;
            debug!(file = %filename, "wrote spec file");

            if filename == "design.md" {
                design_spec.clone_from(content);
            } else if filename == "verification.md" {
                verification_spec.clone_from(content);
            } else if filename == "impl-plan.md" {
                has_impl_plan = true;
            }
        }

        // Step 4: If no impl-plan was generated, request it separately
        let impl_plan_content = if has_impl_plan {
            spec_files
                .iter()
                .find(|(name, _)| name == "impl-plan.md")
                .map(|(_, content)| content.clone())
                .unwrap_or_default()
        } else {
            info!("impl-plan not found in spec output, generating separately");
            let impl_plan_prompt = render_prompt(
                &self.pm,
                "plan-generate-impl-plan",
                &json!({
                    "feature_slug": self.slug,
                    "design_spec": design_spec,
                    "verification_spec": verification_spec,
                }),
            )?;

            let impl_plan_response = self.send_to_client(&impl_plan_prompt).await?;
            let impl_plan_path = specs_dir.join("impl-plan.md");
            write_file(&impl_plan_path, &impl_plan_response).await?;
            debug!("wrote separately generated impl-plan.md");
            impl_plan_response
        };

        // Step 5: Parse task list from impl-plan
        let tasks = build_task_list(&impl_plan_content);
        info!(
            task_count = tasks.len(),
            "generated task list from impl-plan"
        );

        // Step 6: Determine the main branch and create git worktree
        let main_branch = detect_main_branch(&self.ctx.project_root).await;
        let worktree_name = format!("{feature_number:04}_{}", self.slug);
        let worktree_path = self.ctx.trees_dir.join(&worktree_name);
        let branch_name = format!("feat/{feature_number:04}-{}", self.slug);

        create_git_worktree(
            &self.ctx.project_root,
            &worktree_path,
            &branch_name,
            &main_branch,
        )
        .await?;
        info!(
            worktree = %worktree_path.display(),
            branch = %branch_name,
            "created git worktree"
        );

        // Step 7: Build and save state.yml
        let state_path = feature_dir.join("state.yml");
        let state = FeatureState::new(
            FeatureInfo {
                number: feature_number,
                slug: self.slug.clone(),
                branch: branch_name,
                created_at: Utc::now(),
            },
            FeatureStatus::Planned,
            Some(PlanState {
                turns: self.turns,
                cost_usd: self.cost_usd,
                completed_at: Some(Utc::now()),
            }),
            tasks,
            TotalsState {
                turns: self.turns,
                cost_usd: self.cost_usd,
            },
            state_path,
        );
        state.save().await?;
        info!("saved state.yml");

        // Step 8: Disconnect the client
        self.disconnect_client().await;

        Ok(PlanResult {
            feature_number,
            slug: self.slug.clone(),
            turns: self.turns,
            cost_usd: self.cost_usd,
        })
    }

    /// Send a message to the client, accumulating turns and cost.
    async fn send_to_client(&mut self, message: &str) -> Result<String, GbaCoreError> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| GbaCoreError::AgentQuery {
                message: "planning session not started — call start() first".to_owned(),
                source: None,
            })?;

        let result = AgentRunner::send(client, message).await?;
        self.turns = self.turns.saturating_add(result.turns);
        self.cost_usd += result.cost_usd;
        Ok(result.text)
    }

    /// Disconnect the Claude client session gracefully.
    async fn disconnect_client(&mut self) {
        if let Some(mut client) = self.client.take()
            && let Err(e) = client.disconnect().await
        {
            warn!(error = %e, "failed to disconnect client session");
        }
    }
}

/// Parse spec files from the agent's response using `---FILE: <filename>---` separators.
///
/// Returns a list of `(filename, content)` pairs. Content is trimmed of
/// leading/trailing whitespace.
///
/// # Examples
///
/// ```
/// # // This is a doc-test showing the expected format
/// let input = "---FILE: design.md---\n# Design\nSome content\n---FILE: verification.md---\n# Verification\nMore content";
/// // parse_spec_files would return [("design.md", "# Design\nSome content"), ...]
/// ```
fn parse_spec_files(response: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current_filename: Option<String> = None;
    let mut current_content = String::new();

    for line in response.lines() {
        let trimmed = line.trim();
        if let Some(filename) = extract_filename(trimmed) {
            // Save the previous file if any
            if let Some(name) = current_filename.take() {
                let content = current_content.trim().to_owned();
                if !content.is_empty() {
                    files.push((name, content));
                }
            }
            current_filename = Some(filename);
            current_content.clear();
        } else if current_filename.is_some() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Save the last file
    if let Some(name) = current_filename {
        let content = current_content.trim().to_owned();
        if !content.is_empty() {
            files.push((name, content));
        }
    }

    files
}

/// Extract a filename from a `---FILE: <filename>---` line.
///
/// Returns `None` if the line does not match the expected format.
fn extract_filename(line: &str) -> Option<String> {
    let stripped = line.strip_prefix(FILE_SEPARATOR_PREFIX)?;
    let stripped = stripped.strip_suffix(FILE_SEPARATOR_SUFFIX)?;
    let filename = stripped.trim();

    // Validate: reject empty filenames, paths with directory traversal, and
    // absolute paths.
    if filename.is_empty() || filename.contains("..") || filename.starts_with('/') {
        return None;
    }

    // Only allow alphanumeric, hyphens, underscores, dots
    if filename
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Some(filename.to_owned())
    } else {
        None
    }
}

/// Build a task list from an implementation plan document.
///
/// Scans the text for phase headings (lines matching `Phase N` patterns)
/// and generates a structured task list:
/// - One `Setup` task at the start
/// - For each detected phase: a `Build` task followed by a `Commit` task
/// - Trailing tasks: `Verification`, `Review`, `ReviewFix`, `Verification`, `SubmitPr`
///
/// If no phases are detected, returns a minimal default task list.
fn build_task_list(impl_plan: &str) -> Vec<TaskState> {
    let phases = extract_phases(impl_plan);

    if phases.is_empty() {
        return build_minimal_task_list();
    }

    let mut tasks: Vec<TaskState> = Vec::new();
    let mut task_id: u32 = 1;

    // Setup task
    tasks.push(TaskState {
        id: task_id,
        kind: TaskKind::Setup,
        description: "Set up directory structure and scaffolding".to_owned(),
        status: TaskStatus::Pending,
        turns: 0,
        cost_usd: 0.0,
        commit_sha: None,
        completed_at: None,
    });
    task_id = task_id.saturating_add(1);

    // Build + Commit for each phase
    for phase_desc in &phases {
        tasks.push(TaskState {
            id: task_id,
            kind: TaskKind::Build,
            description: phase_desc.clone(),
            status: TaskStatus::Pending,
            turns: 0,
            cost_usd: 0.0,
            commit_sha: None,
            completed_at: None,
        });
        task_id = task_id.saturating_add(1);

        tasks.push(TaskState {
            id: task_id,
            kind: TaskKind::Commit,
            description: format!("Commit: {phase_desc}"),
            status: TaskStatus::Pending,
            turns: 0,
            cost_usd: 0.0,
            commit_sha: None,
            completed_at: None,
        });
        task_id = task_id.saturating_add(1);
    }

    // Trailing tasks
    append_trailing_tasks(&mut tasks, &mut task_id);

    tasks
}

/// Build a minimal task list when phase parsing fails.
fn build_minimal_task_list() -> Vec<TaskState> {
    let mut tasks: Vec<TaskState> = Vec::new();
    let mut task_id: u32 = 1;

    let kinds_and_descriptions: &[(TaskKind, &str)] = &[
        (
            TaskKind::Setup,
            "Set up directory structure and scaffolding",
        ),
        (TaskKind::Build, "Implement feature"),
        (TaskKind::Commit, "Commit implementation"),
    ];

    for (kind, desc) in kinds_and_descriptions {
        tasks.push(TaskState {
            id: task_id,
            kind: kind.clone(),
            description: (*desc).to_owned(),
            status: TaskStatus::Pending,
            turns: 0,
            cost_usd: 0.0,
            commit_sha: None,
            completed_at: None,
        });
        task_id = task_id.saturating_add(1);
    }

    append_trailing_tasks(&mut tasks, &mut task_id);
    tasks
}

/// Append the standard trailing tasks to a task list.
fn append_trailing_tasks(tasks: &mut Vec<TaskState>, task_id: &mut u32) {
    let trailing: &[(TaskKind, &str)] = &[
        (TaskKind::Verification, "Run tests and verification checks"),
        (TaskKind::Review, "Code review"),
        (TaskKind::ReviewFix, "Apply review fixes"),
        (
            TaskKind::Verification,
            "Run post-review verification checks",
        ),
        (TaskKind::SubmitPr, "Push branch and submit pull request"),
    ];

    for (kind, desc) in trailing {
        tasks.push(TaskState {
            id: *task_id,
            kind: kind.clone(),
            description: (*desc).to_owned(),
            status: TaskStatus::Pending,
            turns: 0,
            cost_usd: 0.0,
            commit_sha: None,
            completed_at: None,
        });
        *task_id = task_id.saturating_add(1);
    }
}

/// Extract phase descriptions from an implementation plan.
///
/// Looks for lines matching patterns like:
/// - `## Phase 1: Title`
/// - `### Phase 2 - Title`
/// - `**Phase 3: Title**`
/// - Numbered items at the top level: `1. Title` or `1) Title`
///
/// Heading-style and bold-style phases take priority. If none are found,
/// falls back to numbered list items.
fn extract_phases(text: &str) -> Vec<String> {
    // First pass: look for heading-style and bold-style phases
    let mut phases: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        if let Some(phase_desc) = try_extract_heading_phase(trimmed) {
            phases.push(phase_desc);
            continue;
        }

        if let Some(phase_desc) = try_extract_bold_phase(trimmed) {
            phases.push(phase_desc);
        }
    }

    if !phases.is_empty() {
        return phases;
    }

    // Second pass: fall back to numbered list items
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(phase_desc) = try_extract_numbered_item(trimmed) {
            phases.push(phase_desc);
        }
    }

    phases
}

/// Try to extract a phase from a markdown heading line.
///
/// Matches: `## Phase N: Title`, `### Phase N - Title`, etc.
fn try_extract_heading_phase(line: &str) -> Option<String> {
    // Strip leading '#' characters
    let without_hashes = line.trim_start_matches('#').trim();

    // Check for "Phase N" prefix (case-insensitive)
    let lower = without_hashes.to_lowercase();
    if !lower.starts_with("phase") {
        return None;
    }

    // Find the phase number to confirm this is a phase heading
    let after_phase = without_hashes.get("phase".len()..)?.trim_start();

    // The next non-whitespace chars should start with a digit
    let first_char = after_phase.chars().next()?;
    if !first_char.is_ascii_digit() {
        return None;
    }

    Some(without_hashes.to_owned())
}

/// Try to extract a phase from a bold-wrapped line.
///
/// Matches: `**Phase N: Title**`
fn try_extract_bold_phase(line: &str) -> Option<String> {
    let inner = line.strip_prefix("**")?.strip_suffix("**")?;
    let lower = inner.to_lowercase();

    if !lower.starts_with("phase") {
        return None;
    }

    let after_phase = inner.get("phase".len()..)?.trim_start();
    let first_char = after_phase.chars().next()?;
    if !first_char.is_ascii_digit() {
        return None;
    }

    Some(inner.to_owned())
}

/// Try to extract a phase from a numbered list item.
///
/// Matches: `1. Title` or `1) Title`
fn try_extract_numbered_item(line: &str) -> Option<String> {
    // Find leading digits
    let digit_end = line.find(|c: char| !c.is_ascii_digit())?;
    if digit_end == 0 {
        return None;
    }

    let rest = line.get(digit_end..)?;

    // Must be followed by ". " or ") "
    let title = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;

    let title = title.trim();
    if title.is_empty() {
        return None;
    }

    Some(title.to_owned())
}

/// Read the project summary from `.gba/` analysis results.
///
/// Falls back to a generic message if the summary file does not exist.
async fn read_project_summary(ctx: &GbaContext) -> String {
    // Look for common summary file patterns
    let candidates = [
        ctx.gba_dir.join("summary.md"),
        ctx.gba_dir.join("project-summary.md"),
    ];

    for path in &candidates {
        if let Ok(content) = tokio::fs::read_to_string(path).await
            && !content.trim().is_empty()
        {
            return content;
        }
    }

    // Try to read the root .gba.md for context
    let gba_md_path = ctx.project_root.join(".gba.md");
    if let Ok(content) = tokio::fs::read_to_string(&gba_md_path).await
        && !content.trim().is_empty()
    {
        return content;
    }

    // Fallback: read CLAUDE.md for project context
    let claude_md_path = ctx.project_root.join("CLAUDE.md");
    if let Ok(content) = tokio::fs::read_to_string(&claude_md_path).await
        && !content.trim().is_empty()
    {
        return format!(
            "Project at `{}`. CLAUDE.md content:\n\n{content}",
            ctx.project_root.display()
        );
    }

    format!(
        "Project at `{}`. No detailed summary available.",
        ctx.project_root.display()
    )
}

/// Detect the main branch name for the repository.
///
/// Tries `main` first, then falls back to `master`, then `HEAD`.
async fn detect_main_branch(project_root: &Path) -> String {
    // Try checking symbolic ref of origin/HEAD
    let output = tokio::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(project_root)
        .output()
        .await;

    if let Ok(output) = output
        && output.status.success()
    {
        let ref_str = String::from_utf8_lossy(&output.stdout);
        let branch = ref_str.trim().strip_prefix("refs/remotes/origin/");
        if let Some(branch) = branch
            && !branch.is_empty()
        {
            return branch.to_owned();
        }
    }

    // Check if 'main' branch exists
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--verify", "refs/heads/main"])
        .current_dir(project_root)
        .output()
        .await;

    if let Ok(output) = output
        && output.status.success()
    {
        return "main".to_owned();
    }

    // Fall back to master
    "master".to_owned()
}

/// Create a git worktree for the feature branch.
///
/// Runs: `git worktree add <path> -b <branch> <base_branch>`
async fn create_git_worktree(
    project_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    base_branch: &str,
) -> Result<(), GbaCoreError> {
    let output = tokio::process::Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg(worktree_path)
        .arg("-b")
        .arg(branch_name)
        .arg(base_branch)
        .current_dir(project_root)
        .output()
        .await
        .map_err(|e| GbaCoreError::AgentQuery {
            message: "failed to run git worktree add".to_owned(),
            source: Some(e.into()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GbaCoreError::AgentQuery {
            message: format!("git worktree add failed: {stderr}"),
            source: None,
        });
    }

    Ok(())
}

/// Create a directory (and parents) using `tokio::fs`.
async fn create_dir(path: &Path) -> Result<(), GbaCoreError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| GbaCoreError::StateSave {
            path: path.to_owned(),
            source: e.into(),
        })
}

/// Write content to a file using `tokio::fs`.
async fn write_file(path: &Path, content: &str) -> Result<(), GbaCoreError> {
    tokio::fs::write(path, content.as_bytes())
        .await
        .map_err(|e| GbaCoreError::StateSave {
            path: path.to_owned(),
            source: e.into(),
        })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    // ---------- parse_spec_files ----------

    #[test]
    fn test_should_parse_spec_files_with_three_documents() {
        let response = "\
---FILE: design.md---
# Design

Overview of the feature.

---FILE: verification.md---
# Verification

Test plan here.

---FILE: impl-plan.md---
# Implementation Plan

## Phase 1: Foundation
Build the base.

## Phase 2: Integration
Wire things up.";

        let files = parse_spec_files(response);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].0, "design.md");
        assert!(files[0].1.contains("Overview of the feature"));
        assert_eq!(files[1].0, "verification.md");
        assert!(files[1].1.contains("Test plan here"));
        assert_eq!(files[2].0, "impl-plan.md");
        assert!(files[2].1.contains("Phase 1: Foundation"));
    }

    #[test]
    fn test_should_parse_spec_files_with_leading_text() {
        let response = "\
Here are the spec documents:

---FILE: design.md---
# Design
Content here.

---FILE: verification.md---
# Verification
More content.";

        let files = parse_spec_files(response);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "design.md");
        assert_eq!(files[1].0, "verification.md");
    }

    #[test]
    fn test_should_return_empty_for_no_separators() {
        let response = "This is just plain text with no file separators.";
        let files = parse_spec_files(response);
        assert!(files.is_empty());
    }

    #[test]
    fn test_should_reject_filename_with_directory_traversal() {
        let response = "---FILE: ../etc/passwd---\nevil content";
        let files = parse_spec_files(response);
        assert!(files.is_empty());
    }

    #[test]
    fn test_should_reject_absolute_path_filename() {
        let response = "---FILE: /etc/passwd---\nevil content";
        let files = parse_spec_files(response);
        assert!(files.is_empty());
    }

    #[test]
    fn test_should_reject_empty_filename() {
        let response = "---FILE: ---\nsome content";
        let files = parse_spec_files(response);
        assert!(files.is_empty());
    }

    #[test]
    fn test_should_handle_whitespace_around_filename() {
        let response = "---FILE:  design.md ---\n# Design\nContent.";
        let files = parse_spec_files(response);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "design.md");
    }

    #[test]
    fn test_should_skip_files_with_empty_content() {
        let response = "\
---FILE: empty.md---

---FILE: notempty.md---
Has content.";

        let files = parse_spec_files(response);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "notempty.md");
    }

    // ---------- extract_filename ----------

    #[test]
    fn test_should_extract_valid_filename() {
        assert_eq!(
            extract_filename("---FILE: design.md---"),
            Some("design.md".to_owned())
        );
    }

    #[test]
    fn test_should_extract_filename_with_hyphens_and_underscores() {
        assert_eq!(
            extract_filename("---FILE: impl-plan_v2.md---"),
            Some("impl-plan_v2.md".to_owned())
        );
    }

    #[test]
    fn test_should_reject_filename_with_spaces() {
        assert_eq!(extract_filename("---FILE: my file.md---"), None);
    }

    #[test]
    fn test_should_reject_non_separator_line() {
        assert_eq!(extract_filename("## Phase 1: Setup"), None);
    }

    // ---------- build_task_list ----------

    #[test]
    fn test_should_build_task_list_from_phases() {
        let impl_plan = "\
# Implementation Plan

## Phase 1: Foundation
Build the base module.

## Phase 2: Integration
Wire everything together.

## Phase 3: Polish
Add error handling and tests.";

        let tasks = build_task_list(impl_plan);

        // Setup + (3 phases * 2) + 5 trailing = 1 + 6 + 5 = 12
        assert_eq!(tasks.len(), 12);

        assert_eq!(tasks[0].kind, TaskKind::Setup);
        assert_eq!(tasks[0].id, 1);

        assert_eq!(tasks[1].kind, TaskKind::Build);
        assert!(tasks[1].description.contains("Phase 1: Foundation"));
        assert_eq!(tasks[2].kind, TaskKind::Commit);

        assert_eq!(tasks[3].kind, TaskKind::Build);
        assert!(tasks[3].description.contains("Phase 2: Integration"));
        assert_eq!(tasks[4].kind, TaskKind::Commit);

        assert_eq!(tasks[5].kind, TaskKind::Build);
        assert!(tasks[5].description.contains("Phase 3: Polish"));
        assert_eq!(tasks[6].kind, TaskKind::Commit);

        assert_eq!(tasks[7].kind, TaskKind::Verification);
        assert_eq!(tasks[8].kind, TaskKind::Review);
        assert_eq!(tasks[9].kind, TaskKind::ReviewFix);
        assert_eq!(tasks[10].kind, TaskKind::Verification);
        assert_eq!(tasks[11].kind, TaskKind::SubmitPr);
    }

    #[test]
    fn test_should_build_minimal_task_list_for_empty_plan() {
        let tasks = build_task_list("");

        // Minimal: Setup + Build + Commit + 5 trailing = 8
        assert_eq!(tasks.len(), 8);
        assert_eq!(tasks[0].kind, TaskKind::Setup);
        assert_eq!(tasks[1].kind, TaskKind::Build);
        assert_eq!(tasks[2].kind, TaskKind::Commit);
        assert_eq!(tasks[3].kind, TaskKind::Verification);
        assert_eq!(tasks[7].kind, TaskKind::SubmitPr);
    }

    #[test]
    fn test_should_build_task_list_from_bold_phases() {
        let impl_plan = "\
# Implementation Plan

**Phase 1: Core types**
Define the data structures.

**Phase 2: API layer**
Build the REST endpoints.";

        let tasks = build_task_list(impl_plan);
        // Setup + (2 phases * 2) + 5 trailing = 1 + 4 + 5 = 10
        assert_eq!(tasks.len(), 10);
        assert!(tasks[1].description.contains("Phase 1: Core types"));
        assert!(tasks[3].description.contains("Phase 2: API layer"));
    }

    #[test]
    fn test_should_build_task_list_from_numbered_items() {
        let impl_plan = "\
# Implementation Plan

1. Set up project structure
2. Implement core logic
3. Add tests";

        let tasks = build_task_list(impl_plan);
        // Setup + (3 items * 2) + 5 trailing = 1 + 6 + 5 = 12
        assert_eq!(tasks.len(), 12);
        assert!(tasks[1].description.contains("Set up project structure"));
        assert!(tasks[3].description.contains("Implement core logic"));
        assert!(tasks[5].description.contains("Add tests"));
    }

    #[test]
    fn test_should_assign_sequential_task_ids() {
        let impl_plan = "## Phase 1: A\n## Phase 2: B";
        let tasks = build_task_list(impl_plan);

        for (i, task) in tasks.iter().enumerate() {
            assert_eq!(
                task.id,
                u32::try_from(i).unwrap_or(0).saturating_add(1),
                "task at index {i} should have id {}",
                i + 1
            );
        }
    }

    #[test]
    fn test_should_set_all_tasks_to_pending() {
        let impl_plan = "## Phase 1: A\n## Phase 2: B";
        let tasks = build_task_list(impl_plan);
        for task in &tasks {
            assert_eq!(task.status, TaskStatus::Pending);
        }
    }

    // ---------- extract_phases ----------

    #[test]
    fn test_should_extract_heading_phases() {
        let text = "\
## Phase 1: Foundation
Content.
## Phase 2: Integration
More content.
### Phase 3: Polish
Even more.";

        let phases = extract_phases(text);
        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0], "Phase 1: Foundation");
        assert_eq!(phases[1], "Phase 2: Integration");
        assert_eq!(phases[2], "Phase 3: Polish");
    }

    #[test]
    fn test_should_extract_bold_phases() {
        let text = "**Phase 1: Core**\n**Phase 2: API**";
        let phases = extract_phases(text);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0], "Phase 1: Core");
        assert_eq!(phases[1], "Phase 2: API");
    }

    #[test]
    fn test_should_extract_numbered_items_when_no_headings() {
        let text = "1. First\n2. Second\n3. Third";
        let phases = extract_phases(text);
        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0], "First");
        assert_eq!(phases[1], "Second");
        assert_eq!(phases[2], "Third");
    }

    #[test]
    fn test_should_prefer_heading_phases_over_numbered_items() {
        let text = "\
## Phase 1: Real Phase
Some content.
1. Sub-item one
2. Sub-item two
## Phase 2: Another Phase
More content.";

        let phases = extract_phases(text);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0], "Phase 1: Real Phase");
        assert_eq!(phases[1], "Phase 2: Another Phase");
    }

    #[test]
    fn test_should_return_empty_for_no_phases() {
        let text = "This document has no phase structure at all.";
        let phases = extract_phases(text);
        assert!(phases.is_empty());
    }

    // ---------- feature directory creation ----------

    #[tokio::test]
    async fn test_should_create_feature_directory_structure() {
        let dir = TempDir::new().expect("test: create temp dir");
        let feature_dir = dir.path().join("0001_test-feature");
        let specs_dir = feature_dir.join("specs");

        create_dir(&feature_dir)
            .await
            .expect("test: create feature dir");
        create_dir(&specs_dir)
            .await
            .expect("test: create specs dir");

        assert!(feature_dir.exists());
        assert!(specs_dir.exists());

        write_file(&specs_dir.join("design.md"), "# Design")
            .await
            .expect("test: write file");

        let content = tokio::fs::read_to_string(specs_dir.join("design.md"))
            .await
            .expect("test: read file");
        assert_eq!(content, "# Design");
    }

    // ---------- detect_main_branch ----------

    #[tokio::test]
    async fn test_should_detect_main_branch_in_git_repo() {
        let dir = TempDir::new().expect("test: create temp dir");
        let root = dir.path();

        // Initialize a git repo with 'main' branch
        let init_output = tokio::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .output()
            .await;

        if let Ok(output) = init_output
            && output.status.success()
        {
            // Configure git for the test
            let _ = tokio::process::Command::new("git")
                .args(["config", "user.email", "test@test.com"])
                .current_dir(root)
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(root)
                .output()
                .await;

            // Create an initial commit so refs/heads/main exists
            tokio::fs::write(root.join("README.md"), "# Test")
                .await
                .expect("test: write");
            let _ = tokio::process::Command::new("git")
                .args(["add", "."])
                .current_dir(root)
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(root)
                .output()
                .await;

            let branch = detect_main_branch(root).await;
            assert_eq!(branch, "main");
        }
    }
}
