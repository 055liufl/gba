//! Init engine — analyzes a repository and scaffolds `.gba/` + `.trees/`.
//!
//! The `InitEngine` performs repository analysis by:
//! 1. Scanning the project file tree to build a structural overview
//! 2. Querying the agent to produce a project summary with directory analysis
//! 3. Parsing the agent's JSON response to identify important directories
//! 4. Generating per-directory context documents (`.gba.md`) for key directories
//! 5. Updating `CLAUDE.md` with references to generated context docs
//! 6. Creating the `.gba/` and `.trees/` scaffold directories

use std::path::{Path, PathBuf};

use gba_pm::PromptManager;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::{
    context::GbaContext,
    error::GbaCoreError,
    preset::PresetKind,
    runner::{AgentRunner, render_prompt},
    types::InitResult,
};

/// Maximum directory depth when scanning the file tree.
const MAX_SCAN_DEPTH: u32 = 3;

/// Maximum number of lines in the scanned file tree output.
/// Prevents excessive output when scanning very large repositories.
const MAX_SCAN_LINES: usize = 500;

/// Well-known directory names used as a fallback when the agent's JSON
/// analysis cannot be parsed. The agent's directory analysis is preferred.
const WELL_KNOWN_DIRS: &[&str] = &[
    "src", "crates", "apps", "lib", "packages", "services", "tests", "test", "docs", "config",
    "scripts", "proto", "api", "internal", "cmd", "pkg",
];

/// Directory names to skip when scanning the file tree.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".trees",
    "vendor",
    "vendors",
    ".gba",
    "dist",
    "build",
    "__pycache__",
    ".next",
];

/// Parsed directory entry from the agent's JSON analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnalyzedDirectory {
    /// Relative path to the directory (e.g., "src/", "crates/gba-core").
    path: String,
    /// One-line description of the directory's purpose.
    #[serde(default)]
    description: String,
    /// Importance level: "high", "medium", or "low".
    #[serde(default = "default_importance")]
    importance: String,
}

fn default_importance() -> String {
    "medium".to_owned()
}

/// Parsed JSON response from the agent's repository analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoAnalysis {
    /// Primary programming language.
    #[serde(default)]
    language: String,
    /// Framework used (e.g., "tokio", "actix-web").
    #[serde(default)]
    framework: String,
    /// Build system (e.g., "cargo", "npm").
    #[serde(default)]
    build_system: String,
    /// Brief architecture summary.
    #[serde(default)]
    architecture_summary: String,
    /// Analyzed directories with importance ratings.
    #[serde(default)]
    directories: Vec<AnalyzedDirectory>,
    /// Entry point files.
    #[serde(default)]
    entry_points: Vec<String>,
    /// Observed conventions.
    #[serde(default)]
    conventions: Vec<String>,
}

/// Orchestrates the `gba init` workflow.
///
/// Creates the `.gba/` and `.trees/` directories, analyzes the repository
/// structure via the agent, and generates `.gba.md` context documents for
/// important directories.
#[derive(Debug)]
pub struct InitEngine {
    ctx: GbaContext,
    runner: AgentRunner,
    pm: PromptManager,
}

impl InitEngine {
    /// Create a new `InitEngine` from the given project context.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::PromptRender` if the prompt manager fails to
    /// initialize (should not happen with valid embedded templates).
    pub fn new(ctx: GbaContext) -> Result<Self, GbaCoreError> {
        let runner = AgentRunner::new(
            ctx.project_root.clone(),
            ctx.config.model.clone(),
            ctx.config.max_budget_usd,
        );
        let pm = PromptManager::new().map_err(|e| GbaCoreError::PromptRender {
            template: "<init>".to_owned(),
            source: e,
        })?;
        Ok(Self { ctx, runner, pm })
    }

    /// Execute the full initialization workflow.
    ///
    /// 1. Check whether the project is already initialized.
    /// 2. Create `.gba/` and `.trees/` directories.
    /// 3. Scan the project file tree.
    /// 4. Query the agent to analyze the repository (returns JSON).
    /// 5. Parse the JSON analysis to identify important directories.
    /// 6. Generate `.gba.md` context documents for each important directory.
    /// 7. Append GBA context section (with `.gba.md` references) to `CLAUDE.md`.
    /// 8. Update `.gitignore` to include `.trees/`.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError` on filesystem, prompt rendering, or agent errors.
    pub async fn run(&self) -> Result<InitResult, GbaCoreError> {
        // Step 1: Check if already initialized
        if self.ctx.is_initialized().await {
            return Ok(InitResult {
                performed: false,
                summary: "Already initialized".to_owned(),
                context_doc_count: 0,
            });
        }

        // Step 2: Create scaffold directories
        info!("creating .gba/ and .trees/ directories");
        create_dir(&self.ctx.gba_dir).await?;
        create_dir(&self.ctx.trees_dir).await?;

        // Step 3: Scan the project file tree
        info!("scanning project file tree");
        let file_tree = scan_file_tree(&self.ctx.project_root, MAX_SCAN_DEPTH).await?;
        debug!(lines = file_tree.lines().count(), "file tree scanned");

        // Step 4: Read existing CLAUDE.md for context
        let claude_md = read_optional_file(&self.ctx.project_root.join("CLAUDE.md")).await;

        // Step 5: Render prompts and query the agent for analysis
        info!("analyzing repository structure");
        let system_prompt = render_prompt(&self.pm, "init-system", &json!({}))?;
        let user_prompt = render_prompt(
            &self.pm,
            "init-analyze-repo",
            &json!({
                "project_root": self.ctx.project_root.display().to_string(),
                "file_tree": file_tree,
                "claude_md": claude_md.as_deref().unwrap_or(""),
            }),
        )?;

        let analyze_preset = PresetKind::InitAnalyze.preset();
        let result = self
            .runner
            .query(&analyze_preset, &system_prompt, &user_prompt, None)
            .await?;
        let summary = result.text;

        info!(
            turns = result.turns,
            cost_usd = result.cost_usd,
            "repository analysis complete"
        );

        // Step 6: Parse the JSON analysis result to identify important directories.
        // Falls back to well-known directory names if parsing fails.
        let important_dirs = resolve_important_dirs(&summary, &self.ctx.project_root).await;
        info!(
            count = important_dirs.len(),
            "resolved important directories"
        );

        // Step 7: Persist summary to .gba/summary.md
        let summary_path = self.ctx.gba_dir.join("summary.md");
        write_file(&summary_path, &summary).await?;
        info!(path = %summary_path.display(), "wrote project summary");

        // Step 8: Generate .gba.md context docs for each important directory
        let mut generated_docs: Vec<String> = Vec::new();

        for dir_path in &important_dirs {
            // dir_path was constructed from project_root.join(name), so strip_prefix always
            // succeeds
            let relative = dir_path
                .strip_prefix(&self.ctx.project_root)
                .unwrap_or(dir_path);
            let relative_str = relative.display().to_string();

            debug!(dir = %relative_str, "generating context for directory");

            let file_list = list_directory_files(dir_path).await?;
            let context_prompt = render_prompt(
                &self.pm,
                "init-generate-context",
                &json!({
                    "project_summary": summary,
                    "dir_path": relative_str,
                    "file_list": file_list,
                }),
            )?;

            let gen_preset = PresetKind::InitGenerateContext.preset();
            let context_result = self
                .runner
                .query(&gen_preset, &system_prompt, &context_prompt, None)
                .await?;

            let gba_md_path = dir_path.join(".gba.md");
            write_file(&gba_md_path, &context_result.text).await?;
            debug!(path = %gba_md_path.display(), "wrote context document");

            generated_docs.push(format!("{relative_str}/.gba.md"));
        }

        // Step 9: Append GBA context section to CLAUDE.md (after context docs are generated)
        append_gba_context_to_claude_md(&self.ctx.project_root, &generated_docs).await?;

        // Step 10: Update .gitignore
        update_gitignore(&self.ctx.project_root).await?;

        let context_doc_count = important_dirs.len();

        Ok(InitResult {
            performed: true,
            summary,
            context_doc_count,
        })
    }
}

/// Parse the agent's analysis JSON and resolve directory paths.
///
/// Extracts the `directories` array from the JSON response, filters for
/// directories that exist on disk, and returns their absolute paths.
/// Falls back to [`find_important_dirs`] if JSON parsing fails.
async fn resolve_important_dirs(analysis_text: &str, project_root: &Path) -> Vec<PathBuf> {
    // Try to extract JSON from the agent response. The agent may wrap it in
    // markdown code fences, so try stripping those first.
    let json_str = extract_json_block(analysis_text);

    match serde_json::from_str::<RepoAnalysis>(json_str) {
        Ok(analysis) => {
            let mut dirs = Vec::new();
            for entry in &analysis.directories {
                // Normalize: strip trailing slash
                let dir_name = entry.path.trim_end_matches('/');
                // Reject paths with traversal or absolute paths
                if dir_name.contains("..") || dir_name.starts_with('/') {
                    warn!(path = %dir_name, "skipping directory with unsafe path");
                    continue;
                }
                let path = project_root.join(dir_name);
                if dir_exists(&path).await {
                    dirs.push(path);
                } else {
                    debug!(path = %dir_name, "agent-identified directory does not exist, skipping");
                }
            }
            if dirs.is_empty() {
                warn!(
                    "agent analysis returned no valid directories, falling back to well-known list"
                );
                find_important_dirs(project_root).await
            } else {
                dirs
            }
        }
        Err(e) => {
            warn!(
                error = %e,
                "failed to parse agent analysis JSON, falling back to well-known directory list"
            );
            find_important_dirs(project_root).await
        }
    }
}

/// Extract a JSON block from text that may be wrapped in markdown code fences.
///
/// Handles:
/// - Raw JSON (starts with `{`)
/// - Fenced blocks: ````json ... ``` `` or ```` ``` ... ``` ````
fn extract_json_block(text: &str) -> &str {
    let trimmed = text.trim();

    // Try to find ```json ... ``` or ``` ... ```
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        // Skip optional language tag (e.g., "json")
        let content_start = after_fence.find('\n').map_or(0, |pos| pos + 1);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            return content[..end].trim();
        }
    }

    trimmed
}

/// Check whether a path exists and is a directory, logging warnings on errors.
async fn dir_exists(path: &Path) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(meta) => meta.is_dir(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to check directory existence");
            false
        }
    }
}

/// Create a directory (and parents) using `tokio::fs`.
async fn create_dir(path: &Path) -> Result<(), GbaCoreError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| GbaCoreError::Io {
            path: path.to_owned(),
            source: e.into(),
        })
}

/// Write content to a file using `tokio::fs`.
async fn write_file(path: &Path, content: &str) -> Result<(), GbaCoreError> {
    tokio::fs::write(path, content.as_bytes())
        .await
        .map_err(|e| GbaCoreError::Io {
            path: path.to_owned(),
            source: e.into(),
        })
}

/// Read a file if it exists, returning `None` if it does not.
async fn read_optional_file(path: &Path) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok()
}

/// Recursively scan the project directory to produce a tree-like string.
///
/// Skips hidden directories (except `.github`), `target`, `node_modules`,
/// `.trees`, `vendor`, and other non-essential directories.
/// Limits traversal to `max_depth` levels and output to [`MAX_SCAN_LINES`] lines.
async fn scan_file_tree(root: &Path, max_depth: u32) -> Result<String, GbaCoreError> {
    let mut output = String::with_capacity(4096);
    let mut line_count: usize = 0;
    scan_dir_recursive(root, root, 0, max_depth, &mut output, &mut line_count).await?;
    Ok(output)
}

/// Recursive helper for [`scan_file_tree`].
async fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    depth: u32,
    max_depth: u32,
    output: &mut String,
    line_count: &mut usize,
) -> Result<(), GbaCoreError> {
    if depth >= max_depth || *line_count >= MAX_SCAN_LINES {
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| GbaCoreError::Io {
            path: dir.to_owned(),
            source: e.into(),
        })?;

    let mut items: Vec<(String, bool)> = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| GbaCoreError::Io {
        path: dir.to_owned(),
        source: e.into(),
    })? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        if should_skip_entry(&name_str) {
            continue;
        }

        let file_type = entry.file_type().await.map_err(|e| GbaCoreError::Io {
            path: entry.path(),
            source: e.into(),
        })?;

        items.push((name_str, file_type.is_dir()));
    }

    // Sort: directories first, then alphabetical
    items.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.cmp(&b.0),
    });

    let indent = "  ".repeat(depth as usize);

    for (name, is_dir) in &items {
        if *line_count >= MAX_SCAN_LINES {
            let remaining = items
                .len()
                .saturating_sub(items.iter().position(|(n, _)| n == name).unwrap_or(0));
            output.push_str(&format!(
                "{indent}... (truncated, {remaining} more entries)\n"
            ));
            *line_count = line_count.saturating_add(1);
            break;
        }

        if *is_dir {
            output.push_str(&indent);
            output.push_str(name);
            output.push_str("/\n");
            *line_count = line_count.saturating_add(1);

            let child_path = dir.join(name);
            Box::pin(scan_dir_recursive(
                root,
                &child_path,
                depth.saturating_add(1),
                max_depth,
                output,
                line_count,
            ))
            .await?;
        } else {
            output.push_str(&indent);
            output.push_str(name);
            output.push('\n');
            *line_count = line_count.saturating_add(1);
        }
    }

    Ok(())
}

/// Determine whether a directory entry should be skipped during scanning.
fn should_skip_entry(name: &str) -> bool {
    // Allow `.github` but skip other hidden dirs/files
    if name.starts_with('.') && name != ".github" {
        return true;
    }

    SKIP_DIRS.contains(&name)
}

/// Find important directories using the well-known directory list.
///
/// This is the fallback path when the agent's JSON analysis cannot be parsed.
/// Checks for the existence of well-known directory names at the project root.
async fn find_important_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for &name in WELL_KNOWN_DIRS {
        let path = root.join(name);
        if dir_exists(&path).await {
            dirs.push(path);
        }
    }

    dirs
}

/// List files in a directory (non-recursive, one level) as a newline-separated string.
async fn list_directory_files(dir: &Path) -> Result<String, GbaCoreError> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| GbaCoreError::Io {
            path: dir.to_owned(),
            source: e.into(),
        })?;

    let mut names: Vec<String> = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| GbaCoreError::Io {
        path: dir.to_owned(),
        source: e.into(),
    })? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        let file_type = entry.file_type().await.map_err(|e| GbaCoreError::Io {
            path: entry.path(),
            source: e.into(),
        })?;

        if file_type.is_dir() {
            names.push(format!("{name_str}/"));
        } else {
            names.push(name_str);
        }
    }

    names.sort();
    Ok(names.join("\n"))
}

/// Build the GBA context section for `CLAUDE.md`, including references to
/// generated `.gba.md` files.
fn build_gba_context_section(generated_docs: &[String]) -> String {
    let mut section = String::from(
        "\n\n## GBA Context\n\nThis project uses GBA for AI-assisted feature development. See \
         `.gba/` for project context.\n",
    );

    if !generated_docs.is_empty() {
        section.push_str("\nPer-directory context docs:\n");
        for doc_path in generated_docs {
            section.push_str(&format!("- `{doc_path}`\n"));
        }
    }

    section
}

/// Append a GBA context section to `CLAUDE.md` in the project root.
///
/// Creates the file if it does not exist. If the file already contains the
/// GBA context header, it is left unchanged.
async fn append_gba_context_to_claude_md(
    project_root: &Path,
    generated_docs: &[String],
) -> Result<(), GbaCoreError> {
    let claude_md_path = project_root.join("CLAUDE.md");
    let existing = read_optional_file(&claude_md_path).await;
    let section = build_gba_context_section(generated_docs);

    match existing {
        Some(content) => {
            if content.contains("## GBA Context") {
                debug!("CLAUDE.md already contains GBA context section");
            } else {
                let mut new_content = content;
                new_content.push_str(&section);
                write_file(&claude_md_path, &new_content).await?;
                debug!("appended GBA context section to CLAUDE.md");
            }
        }
        None => {
            write_file(&claude_md_path, section.trim_start()).await?;
            debug!("created CLAUDE.md with GBA context section");
        }
    }

    Ok(())
}

/// Ensure `.trees/` is listed in the project's `.gitignore`.
///
/// Creates the file if it does not exist. Appends `.trees/` only if not
/// already present.
async fn update_gitignore(project_root: &Path) -> Result<(), GbaCoreError> {
    let gitignore_path = project_root.join(".gitignore");
    let trees_entry = ".trees/";

    let existing = read_optional_file(&gitignore_path).await;

    match existing {
        Some(content) => {
            // Check if .trees/ is already listed (as a standalone line)
            let already_present = content.lines().any(|line| line.trim() == trees_entry);
            if !already_present {
                let mut new_content = content;
                // Ensure trailing newline before appending
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                new_content.push_str(trees_entry);
                new_content.push('\n');
                write_file(&gitignore_path, &new_content).await?;
                debug!("appended .trees/ to .gitignore");
            }
        }
        None => {
            write_file(&gitignore_path, &format!("{trees_entry}\n")).await?;
            debug!("created .gitignore with .trees/ entry");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn setup_project_dir() -> TempDir {
        let dir = TempDir::new().expect("test: failed to create temp dir");
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .expect("test: failed to create .git");
        dir
    }

    #[tokio::test]
    async fn test_should_skip_hidden_dirs_except_github() {
        assert!(should_skip_entry(".git"));
        assert!(should_skip_entry(".hidden"));
        assert!(!should_skip_entry(".github"));
        assert!(!should_skip_entry("src"));
    }

    #[tokio::test]
    async fn test_should_skip_well_known_ignored_dirs() {
        assert!(should_skip_entry("target"));
        assert!(should_skip_entry("node_modules"));
        assert!(should_skip_entry(".trees"));
        assert!(should_skip_entry("vendor"));
        assert!(should_skip_entry("vendors"));
    }

    #[tokio::test]
    async fn test_should_scan_file_tree_with_depth_limit() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        // Create a nested structure
        tokio::fs::create_dir_all(root.join("src/models/nested"))
            .await
            .expect("test: mkdir");
        tokio::fs::write(root.join("src/main.rs"), "fn main() {}")
            .await
            .expect("test: write");
        tokio::fs::write(root.join("src/lib.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::write(root.join("src/models/user.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::write(root.join("src/models/nested/deep.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::write(root.join("Cargo.toml"), "")
            .await
            .expect("test: write");

        let tree = scan_file_tree(root, 3).await.expect("scan should succeed");

        // Should contain top-level entries
        assert!(tree.contains("src/"));
        assert!(tree.contains("Cargo.toml"));
        // Should contain depth-1 entries
        assert!(tree.contains("main.rs"));
        assert!(tree.contains("models/"));
        // Should contain depth-2 entries
        assert!(tree.contains("user.rs"));
        // Should NOT contain depth-3 entries (nested/deep.rs is at depth 3)
        assert!(!tree.contains("deep.rs"));
    }

    #[tokio::test]
    async fn test_should_skip_git_and_target_dirs_in_scan() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");
        tokio::fs::create_dir(root.join("target"))
            .await
            .expect("test: mkdir");
        tokio::fs::write(root.join("src/lib.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::write(root.join("target/debug"), "")
            .await
            .expect("test: write");

        let tree = scan_file_tree(root, 3).await.expect("scan should succeed");

        assert!(tree.contains("src/"));
        assert!(!tree.contains("target"));
        // .git is hidden so also skipped
        assert!(!tree.contains(".git"));
    }

    #[tokio::test]
    async fn test_should_find_important_dirs() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");
        tokio::fs::create_dir(root.join("crates"))
            .await
            .expect("test: mkdir");
        tokio::fs::create_dir(root.join("docs"))
            .await
            .expect("test: mkdir");
        // A file named like a well-known dir should not be included
        tokio::fs::write(root.join("lib"), "not a dir")
            .await
            .expect("test: write");

        let dirs = find_important_dirs(root).await;

        let names: Vec<&str> = dirs
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();

        assert!(names.contains(&"src"));
        assert!(names.contains(&"crates"));
        assert!(names.contains(&"docs"));
        assert!(!names.contains(&"lib")); // file, not dir
    }

    #[tokio::test]
    async fn test_should_list_directory_files() {
        let dir = setup_project_dir().await;
        let src = dir.path().join("src");
        tokio::fs::create_dir(&src).await.expect("test: mkdir");
        tokio::fs::write(src.join("main.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::write(src.join("lib.rs"), "")
            .await
            .expect("test: write");
        tokio::fs::create_dir(src.join("models"))
            .await
            .expect("test: mkdir");

        let listing = list_directory_files(&src)
            .await
            .expect("listing should succeed");

        assert!(listing.contains("main.rs"));
        assert!(listing.contains("lib.rs"));
        assert!(listing.contains("models/"));
    }

    #[tokio::test]
    async fn test_should_create_gitignore_when_missing() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        update_gitignore(root).await.expect("should succeed");

        let content = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .expect("test: read");
        assert!(content.contains(".trees/"));
    }

    #[tokio::test]
    async fn test_should_append_to_existing_gitignore() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::write(root.join(".gitignore"), "target/\n")
            .await
            .expect("test: write");

        update_gitignore(root).await.expect("should succeed");

        let content = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .expect("test: read");
        assert!(content.contains("target/"));
        assert!(content.contains(".trees/"));
    }

    #[tokio::test]
    async fn test_should_not_duplicate_trees_in_gitignore() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::write(root.join(".gitignore"), "target/\n.trees/\n")
            .await
            .expect("test: write");

        update_gitignore(root).await.expect("should succeed");

        let content = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .expect("test: read");
        // Should appear exactly once
        let count = content.matches(".trees/").count();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_should_handle_gitignore_without_trailing_newline() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::write(root.join(".gitignore"), "target/")
            .await
            .expect("test: write");

        update_gitignore(root).await.expect("should succeed");

        let content = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .expect("test: read");
        assert!(content.contains("target/\n.trees/\n"));
    }

    #[tokio::test]
    async fn test_should_read_optional_file_returns_none_when_missing() {
        let result = read_optional_file(Path::new("/nonexistent/file.txt")).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_should_read_optional_file_returns_content() {
        let dir = TempDir::new().expect("test: create temp dir");
        let path = dir.path().join("test.txt");
        tokio::fs::write(&path, "hello world")
            .await
            .expect("test: write");

        let result = read_optional_file(&path).await;
        assert_eq!(result.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn test_should_sort_tree_entries_dirs_first() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::write(root.join("zebra.txt"), "")
            .await
            .expect("test: write");
        tokio::fs::create_dir(root.join("alpha"))
            .await
            .expect("test: mkdir");
        tokio::fs::write(root.join("beta.txt"), "")
            .await
            .expect("test: write");

        let tree = scan_file_tree(root, 2).await.expect("scan should succeed");
        let lines: Vec<&str> = tree.lines().collect();

        // Directories should come before files
        let alpha_pos = lines.iter().position(|l| l.contains("alpha/"));
        let beta_pos = lines.iter().position(|l| l.contains("beta.txt"));
        let zebra_pos = lines.iter().position(|l| l.contains("zebra.txt"));

        assert!(alpha_pos.is_some());
        assert!(beta_pos.is_some());
        assert!(zebra_pos.is_some());

        // alpha/ (dir) should come before both files
        assert!(alpha_pos < beta_pos);
        assert!(alpha_pos < zebra_pos);
    }

    #[tokio::test]
    async fn test_should_parse_agent_analysis_json() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        // Create directories the agent would identify
        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");
        tokio::fs::create_dir(root.join("tests"))
            .await
            .expect("test: mkdir");

        let json = r#"{
            "language": "Rust",
            "framework": "tokio",
            "build_system": "cargo",
            "architecture_summary": "A CLI tool",
            "directories": [
                {"path": "src/", "description": "Main source code", "importance": "high"},
                {"path": "tests/", "description": "Test files", "importance": "medium"}
            ],
            "entry_points": ["src/main.rs"],
            "conventions": ["snake_case"]
        }"#;

        let dirs = resolve_important_dirs(json, root).await;
        let names: Vec<&str> = dirs
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();

        assert!(names.contains(&"src"));
        assert!(names.contains(&"tests"));
    }

    #[tokio::test]
    async fn test_should_fallback_to_well_known_dirs_on_invalid_json() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");

        let invalid_json = "This is not valid JSON at all.";
        let dirs = resolve_important_dirs(invalid_json, root).await;

        let names: Vec<&str> = dirs
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();

        assert!(names.contains(&"src"));
    }

    #[tokio::test]
    async fn test_should_extract_json_from_code_fence() {
        let text = r#"Here is my analysis:

```json
{"language": "Rust", "directories": []}
```

Done!"#;

        let extracted = extract_json_block(text);
        assert!(extracted.starts_with('{'));
        assert!(extracted.ends_with('}'));

        let parsed: serde_json::Result<RepoAnalysis> = serde_json::from_str(extracted);
        assert!(parsed.is_ok());
    }

    #[tokio::test]
    async fn test_should_reject_directory_traversal_in_analysis() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");

        let json = r#"{
            "language": "Rust",
            "directories": [
                {"path": "../etc/passwd", "description": "malicious", "importance": "high"},
                {"path": "src/", "description": "source", "importance": "high"}
            ]
        }"#;

        let dirs = resolve_important_dirs(json, root).await;
        let names: Vec<&str> = dirs
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();

        assert!(names.contains(&"src"));
        assert!(!names.iter().any(|n| n.contains("passwd")));
    }

    #[tokio::test]
    async fn test_should_fallback_when_no_valid_dirs_in_analysis() {
        let dir = setup_project_dir().await;
        let root = dir.path();

        tokio::fs::create_dir(root.join("src"))
            .await
            .expect("test: mkdir");

        let json = r#"{
            "language": "Rust",
            "directories": [
                {"path": "nonexistent-dir/", "description": "does not exist", "importance": "high"}
            ]
        }"#;

        let dirs = resolve_important_dirs(json, root).await;
        let names: Vec<&str> = dirs
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();

        // Should fall back to well-known dirs
        assert!(names.contains(&"src"));
    }

    #[tokio::test]
    async fn test_should_build_gba_context_section_with_docs() {
        let docs = vec!["src/.gba.md".to_owned(), "crates/.gba.md".to_owned()];

        let section = build_gba_context_section(&docs);

        assert!(section.contains("## GBA Context"));
        assert!(section.contains("Per-directory context docs:"));
        assert!(section.contains("- `src/.gba.md`"));
        assert!(section.contains("- `crates/.gba.md`"));
    }

    #[tokio::test]
    async fn test_should_build_gba_context_section_without_docs() {
        let section = build_gba_context_section(&[]);
        assert!(section.contains("## GBA Context"));
        assert!(!section.contains("Per-directory"));
    }
}
