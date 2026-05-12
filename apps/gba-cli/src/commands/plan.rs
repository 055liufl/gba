//! Handler for the `gba plan` command.

use tracing::info;

/// Execute the plan command.
///
/// # Errors
///
/// Returns an error if planning fails.
pub async fn execute(slug: &str) -> anyhow::Result<()> {
    info!(slug, "plan command not yet implemented");
    Ok(())
}
