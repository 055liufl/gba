//! Handler for the `gba run` command.

use gba_core::{GbaContext, RunEngine, TaskStatus};
use tracing::info;

/// Execute the run command.
///
/// Loads the project context, creates a `RunEngine`, and executes all
/// remaining tasks for the given feature slug.
///
/// # Errors
///
/// Returns an error if the project is not initialized or the run fails.
pub async fn execute(slug: &str) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let ctx = GbaContext::load(&cwd).await?;
    if !ctx.is_initialized().await {
        anyhow::bail!("Project not initialized. Run `gba init` first.");
    }

    let engine = RunEngine::new(ctx, slug.to_owned())?;
    let result = engine
        .run(|progress| {
            let status_icon = match progress.status {
                TaskStatus::Completed => "\u{2713}",
                TaskStatus::Failed => "\u{2717}",
                TaskStatus::Running => "\u{2192}",
                _ => " ",
            };
            info!(
                "[{status_icon}] {}: {}",
                progress.kind_label(),
                progress.description
            );
        })
        .await?;

    if result.pr_url.is_empty() {
        info!(
            "Run completed. {} turns, ${:.2}",
            result.total_turns, result.total_cost_usd
        );
    } else {
        info!("PR created: {}", result.pr_url);
        info!(
            "Total: {} turns, ${:.2}",
            result.total_turns, result.total_cost_usd
        );
    }

    Ok(())
}
