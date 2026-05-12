//! Handler for the `gba list` command.

use tracing::info;

/// Execute the list command.
///
/// # Errors
///
/// Returns an error if listing fails.
pub async fn execute() -> anyhow::Result<()> {
    info!("list command not yet implemented");
    Ok(())
}
