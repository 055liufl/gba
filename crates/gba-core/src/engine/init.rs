//! Init engine — analyzes a repository and scaffolds `.gba/` + `.trees/`.
//!
//! The `InitEngine` performs repository analysis by:
//! 1. Scanning the project file tree to build a structural overview
//! 2. Querying the agent to produce a project summary
//! 3. Generating per-directory context documents (`.gba.md`) for key directories
//! 4. Creating the `.gba/` and `.trees/` scaffold directories

use std::path::{Path, PathBuf};

use gba_pm::PromptManager;
use serde_json::json;
use tracing::{debug, info};

use crate::{
    context::GbaContext,
    error::GbaCoreError,
    preset::PresetKind,
    runner::{AgentRunner, render_prompt},
    types::InitResult,
};

/// Maximum directory depth when scanning the file tree.
const MAX_SCAN_DEPTH: u32 = 3;

/// Well-known directory names that are considered important for context generation.
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
    /// 4. Query the agent to analyze the repository.
    /// 5. Generate `.gba.md` context documents for important directories.
    /// 6. Update `.gitignore` to include `.trees/`.
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

        // Step 6: Identify important directories and generate context docs
        let important_dirs = find_important_dirs(&self.ctx.project_root).await;
        info!(count = important_dirs.len(), "found important directories");

        for dir_path in &important_dirs {
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
        }

        // Step 7: Update .gitignore
        update_gitignore(&self.ctx.project_root).await?;

        Ok(InitResult {
            performed: true,
            summary,
        })
    }
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

/// Read a file if it exists, returning `None` if it does not.
async fn read_optional_file(path: &Path) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok()
}

/// Recursively scan the project directory to produce a tree-like string.
///
/// Skips hidden directories (except `.github`), `target`, `node_modules`,
/// `.trees`, `vendor`, and other non-essential directories.
/// Limits traversal to `max_depth` levels.
async fn scan_file_tree(root: &Path, max_depth: u32) -> Result<String, GbaCoreError> {
    let mut output = String::with_capacity(4096);
    scan_dir_recursive(root, root, 0, max_depth, &mut output).await?;
    Ok(output)
}

/// Recursive helper for [`scan_file_tree`].
async fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    depth: u32,
    max_depth: u32,
    output: &mut String,
) -> Result<(), GbaCoreError> {
    if depth >= max_depth {
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: dir.to_owned(),
            source: e.into(),
        })?;

    let mut items: Vec<(String, bool)> = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: dir.to_owned(),
            source: e.into(),
        })?
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        if should_skip_entry(&name_str) {
            continue;
        }

        let file_type = entry
            .file_type()
            .await
            .map_err(|e| GbaCoreError::StateLoad {
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
        if *is_dir {
            output.push_str(&indent);
            output.push_str(name);
            output.push_str("/\n");

            let child_path = dir.join(name);
            Box::pin(scan_dir_recursive(
                root,
                &child_path,
                depth.saturating_add(1),
                max_depth,
                output,
            ))
            .await?;
        } else {
            output.push_str(&indent);
            output.push_str(name);
            output.push('\n');
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

/// Find important directories in the project root that should receive
/// `.gba.md` context documents.
///
/// Checks for the existence of well-known directory names at the project root.
async fn find_important_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for &name in WELL_KNOWN_DIRS {
        let path = root.join(name);
        if tokio::fs::try_exists(&path).await.unwrap_or(false)
            && let Ok(meta) = tokio::fs::metadata(&path).await
            && meta.is_dir()
        {
            dirs.push(path);
        }
    }

    dirs
}

/// List files in a directory (non-recursive, one level) as a newline-separated string.
async fn list_directory_files(dir: &Path) -> Result<String, GbaCoreError> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: dir.to_owned(),
            source: e.into(),
        })?;

    let mut names: Vec<String> = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| GbaCoreError::StateLoad {
            path: dir.to_owned(),
            source: e.into(),
        })?
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        let file_type = entry
            .file_type()
            .await
            .map_err(|e| GbaCoreError::StateLoad {
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
}
