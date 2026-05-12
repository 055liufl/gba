//! Error types for the GBA Prompt Manager.

use thiserror::Error;

/// Errors that can occur in the prompt manager.
#[derive(Debug, Error)]
pub enum GbaPmError {
    /// The requested template was not found in the registry.
    #[error("template not found: {name}")]
    TemplateNotFound {
        /// Name of the template that was not found.
        name: String,
    },

    /// An error occurred while rendering a template.
    #[error("failed to render template '{name}': {source}")]
    RenderError {
        /// Name of the template that failed to render.
        name: String,
        /// The underlying MiniJinja error.
        #[source]
        source: minijinja::Error,
    },
}
