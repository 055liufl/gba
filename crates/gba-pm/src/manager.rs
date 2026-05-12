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
}
