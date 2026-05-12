//! Handler for the `gba init` command.

use gba_core::{GbaContext, InitEngine};

/// Execute the init command.
///
/// Discovers the project context, creates an `InitEngine`, and runs the full
/// initialization workflow. Reports the outcome to the user.
///
/// # Errors
///
/// Returns an error if context discovery or initialization fails.
pub async fn execute(model: Option<&str>, budget: Option<f64>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut ctx = GbaContext::load(&cwd).await?;
    ctx.apply_overrides(model, budget);
    let engine = InitEngine::new(ctx)?;
    let result = engine.run().await?;

    if result.performed {
        println!(
            "GBA initialized: {} context document(s) generated.",
            result.context_doc_count
        );
    } else {
        println!("Already initialized, nothing to do.");
    }

    Ok(())
}
