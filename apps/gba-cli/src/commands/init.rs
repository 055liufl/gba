//! Handler for the `gba init` command.

use gba_core::{GbaContext, InitEngine};
use tracing::info;

/// Execute the init command.
///
/// Discovers the project context, creates an `InitEngine`, and runs the full
/// initialization workflow. Reports the outcome to the user.
///
/// # Errors
///
/// Returns an error if context discovery or initialization fails.
pub async fn execute() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let ctx = GbaContext::load(&cwd).await?;
    let engine = InitEngine::new(ctx)?;
    let result = engine.run().await?;

    if result.performed {
        info!("GBA initialized successfully");
        info!("Project analysis:\n{}", result.summary);
    } else {
        info!("GBA already initialized, skipping");
    }

    Ok(())
}
