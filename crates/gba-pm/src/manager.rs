//! Prompt manager — loads, manages, and renders prompt templates.

use minijinja::{Environment, UndefinedBehavior};
use serde_json::Value;

use crate::{error::GbaPmError, templates::TEMPLATES};

/// Manages prompt templates and renders them with runtime context.
///
/// Templates are embedded at compile time and registered into a MiniJinja
/// environment on construction.
///
/// # Examples
///
/// ```
/// use gba_pm::PromptManager;
/// use serde_json::json;
///
/// let pm = PromptManager::new().unwrap();
/// let rendered = pm.render("init-system", &json!({})).unwrap();
/// assert!(!rendered.is_empty());
/// ```
#[derive(Debug)]
pub struct PromptManager {
    env: Environment<'static>,
}

impl PromptManager {
    /// Create a new `PromptManager` with all embedded templates registered.
    ///
    /// # Errors
    ///
    /// Returns `GbaPmError::RenderError` if a template fails to parse during
    /// registration (should not happen with valid compile-time templates).
    pub fn new() -> Result<Self, GbaPmError> {
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);

        for &(name, source) in TEMPLATES {
            env.add_template_owned(name.to_owned(), source.to_owned())
                .map_err(|e| GbaPmError::RenderError {
                    name: name.to_owned(),
                    source: e,
                })?;
        }

        Ok(Self { env })
    }

    /// Render a template by name with the given JSON context.
    ///
    /// # Errors
    ///
    /// - `GbaPmError::TemplateNotFound` if the template name is not registered.
    /// - `GbaPmError::RenderError` if rendering fails (e.g., missing required variable).
    pub fn render(&self, name: &str, ctx: &Value) -> Result<String, GbaPmError> {
        let tmpl = self
            .env
            .get_template(name)
            .map_err(|_| GbaPmError::TemplateNotFound {
                name: name.to_owned(),
            })?;

        tmpl.render(ctx).map_err(|e| GbaPmError::RenderError {
            name: name.to_owned(),
            source: e,
        })
    }

    /// Return the names of all registered templates.
    pub fn list_templates(&self) -> Vec<&str> {
        let mut names: Vec<&str> = TEMPLATES.iter().map(|&(name, _)| name).collect();
        names.sort_unstable();
        names
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_create_prompt_manager() {
        let pm = PromptManager::new();
        assert!(pm.is_ok());
    }

    #[test]
    fn test_should_render_template_with_valid_context() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "project_root": "/home/user/myproject",
            "file_tree": "src/\n  main.rs\n  lib.rs",
            "claude_md": ""
        });
        let result = pm.render("init-analyze-repo", &ctx);
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("/home/user/myproject"));
        assert!(rendered.contains("main.rs"));
    }

    #[test]
    fn test_should_render_template_with_conditional_variable() {
        let pm = PromptManager::new().unwrap();

        // Without claude_md (falsy empty string — Jinja treats "" as falsy)
        let ctx = json!({
            "project_root": "/root",
            "file_tree": "src/",
            "claude_md": ""
        });
        let result = pm.render("init-analyze-repo", &ctx).unwrap();
        assert!(!result.contains("Existing CLAUDE.md"));

        // With claude_md
        let ctx_with_md = json!({
            "project_root": "/root",
            "file_tree": "src/",
            "claude_md": "# My Project\nSome rules here"
        });
        let result_with_md = pm.render("init-analyze-repo", &ctx_with_md).unwrap();
        assert!(result_with_md.contains("Existing CLAUDE.md"));
    }

    #[test]
    fn test_should_return_error_for_unknown_template() {
        let pm = PromptManager::new().unwrap();
        let result = pm.render("nonexistent-template", &json!({}));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaPmError::TemplateNotFound { .. }
        ));
    }

    #[test]
    fn test_should_return_error_for_missing_required_variable() {
        let pm = PromptManager::new().unwrap();
        // plan-start-conversation requires feature_slug
        let result = pm.render("plan-start-conversation", &json!({}));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaPmError::RenderError { .. }
        ));
    }

    #[test]
    fn test_should_list_all_templates() {
        let pm = PromptManager::new().unwrap();
        let templates = pm.list_templates();
        assert_eq!(templates.len(), 15);
        assert!(templates.contains(&"init-system"));
        assert!(templates.contains(&"plan-system"));
        assert!(templates.contains(&"run-system"));
        assert!(templates.contains(&"run-build-phase"));
        assert!(templates.contains(&"run-submit-pr"));
    }

    #[test]
    fn test_should_render_plan_system_with_project_summary() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "project_summary": "A Rust CLI tool for managing features via AI agents."
        });
        let result = pm.render("plan-system", &ctx).unwrap();
        assert!(result.contains("software architect"));
        assert!(result.contains("Rust CLI tool"));
    }

    #[test]
    fn test_should_render_run_system_with_feature_slug() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({ "feature_slug": "observer-module" });
        let result = pm.render("run-system", &ctx).unwrap();
        assert!(result.contains("observer-module"));
        assert!(result.contains("senior software engineer"));
    }

    #[test]
    fn test_should_render_run_build_phase_with_realistic_context() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "phase_number": "2",
            "phase_description": "Phase 2: Integration layer",
            "design_spec": "# Design\n\nIntegrate the observer with the event bus.",
            "impl_plan": "## Phase 2: Integration\n\nWire observer into event bus.",
            "verification_spec": "## Tests\n\n- Unit test for event dispatch."
        });
        let result = pm.render("run-build-phase", &ctx).unwrap();
        assert!(result.contains("phase 2"));
        assert!(result.contains("Integration layer"));
        assert!(result.contains("observer with the event bus"));
        assert!(result.contains("Wire observer"));
        assert!(result.contains("event dispatch"));
    }

    #[test]
    fn test_should_render_run_build_phase_resume() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "phase_number": "1",
            "phase_description": "Phase 1: Foundation",
            "previous_status": "Running",
            "design_spec": "# Design\n\nBase module.",
            "impl_plan": "## Phase 1\nCreate types.",
            "verification_spec": "## Tests\nUnit tests."
        });
        let result = pm.render("run-build-phase-resume", &ctx).unwrap();
        assert!(result.contains("RESUME"));
        assert!(result.contains("Running"));
        assert!(result.contains("Foundation"));
    }

    #[test]
    fn test_should_render_run_commit() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "feature_slug": "web-frontend",
            "phase_number": "3",
            "phase_description": "Polish and error handling"
        });
        let result = pm.render("run-commit", &ctx).unwrap();
        assert!(result.contains("web-frontend"));
        assert!(result.contains("phase 3"));
        assert!(result.contains("Conventional Commits"));
    }

    #[test]
    fn test_should_render_run_verification() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "feature_slug": "api-layer",
            "verification_spec": "## Unit Tests\n- Test endpoint handlers\n- Test error responses"
        });
        let result = pm.render("run-verification", &ctx).unwrap();
        assert!(result.contains("api-layer"));
        assert!(result.contains("Test endpoint handlers"));
        assert!(result.contains("cargo build"));
        assert!(result.contains("cargo test"));
    }

    #[test]
    fn test_should_render_run_review() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "feature_slug": "auth-module",
            "design_spec": "# Design\nAuth module with JWT tokens.",
            "verification_spec": "# Tests\nTest token validation.",
            "all_changes_diff": "+fn validate_token(token: &str) -> Result<Claims> {"
        });
        let result = pm.render("run-review", &ctx).unwrap();
        assert!(result.contains("auth-module"));
        assert!(result.contains("JWT tokens"));
        assert!(result.contains("validate_token"));
        assert!(result.contains("CRITICAL"));
    }

    #[test]
    fn test_should_render_run_review_fix() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "feature_slug": "cache-layer",
            "review_findings": "## [WARNING] Missing error handling\n**File**: `src/cache.rs:42`",
            "design_spec": "# Design\nIn-memory cache with TTL."
        });
        let result = pm.render("run-review-fix", &ctx).unwrap();
        assert!(result.contains("cache-layer"));
        assert!(result.contains("Missing error handling"));
        assert!(result.contains("CRITICAL"));
    }

    #[test]
    fn test_should_render_run_submit_pr() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "feature_slug": "search-feature",
            "design_spec": "# Design\nFull-text search using Tantivy."
        });
        let result = pm.render("run-submit-pr", &ctx).unwrap();
        assert!(result.contains("search-feature"));
        assert!(result.contains("gh pr create"));
        assert!(result.contains("Tantivy"));
    }

    #[test]
    fn test_should_render_plan_generate_spec() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({ "feature_slug": "notifications" });
        let result = pm.render("plan-generate-spec", &ctx).unwrap();
        assert!(result.contains("notifications"));
        assert!(result.contains("design.md"));
        assert!(result.contains("verification.md"));
        assert!(result.contains("impl-plan.md"));
    }

    #[test]
    fn test_should_render_init_generate_context() {
        let pm = PromptManager::new().unwrap();
        let ctx = json!({
            "project_summary": "A microservices platform.",
            "dir_path": "src/services",
            "file_list": "auth.rs\ncache.rs\nmod.rs"
        });
        let result = pm.render("init-generate-context", &ctx).unwrap();
        assert!(result.contains("src/services"));
        assert!(result.contains("auth.rs"));
    }
}
