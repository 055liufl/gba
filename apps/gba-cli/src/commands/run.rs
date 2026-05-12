//! Handler for the `gba run` command.

use tracing::info;

/// Execute the run command.
///
/// # Errors
///
/// Returns an error if the run fails.
pub async fn execute(slug: &str) -> anyhow::Result<()> {
    info!(slug, "run command not yet implemented");
    Ok(())
}
