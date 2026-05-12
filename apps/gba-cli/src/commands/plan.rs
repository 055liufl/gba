//! Handler for the `gba plan` command.
//!
//! Opens a multi-turn planning conversation for the given feature slug,
//! reads user input from stdin, and finalizes the plan into specs, a
//! task list, and a git worktree.

use gba_core::{GbaContext, PlanEngine};
use tokio::io::AsyncBufReadExt;
use tracing::info;

/// Execute the plan command.
///
/// Discovers the project context, creates a `PlanEngine`, runs an interactive
/// conversation loop, and finalizes the plan.
///
/// # Errors
///
/// Returns an error if context discovery, engine initialization, or the
/// planning workflow fails.
pub async fn execute(slug: &str) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let ctx = GbaContext::load(&cwd).await?;
    if !ctx.is_initialized().await {
        anyhow::bail!("Project not initialized. Run `gba init` first.");
    }

    let mut engine = PlanEngine::new(ctx, slug.to_owned())?;
    let response = engine.start().await?;
    println!("\nAssistant: {response}\n");

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        eprint!("You: ");

        let line = match lines.next_line().await? {
            Some(line) => line,
            None => break, // EOF
        };

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "/done" {
            break;
        }

        let response = engine.send(trimmed).await?;
        println!("\nAssistant: {response}\n");
    }

    info!("Finalizing plan...");
    let result = engine.finalize().await?;
    info!(
        "Feature #{:04} '{}' planned successfully",
        result.feature_number, result.slug
    );
    info!(
        "Planning used {} turns, ${:.2}",
        result.turns, result.cost_usd
    );

    Ok(())
}
