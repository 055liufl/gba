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
pub async fn execute(
    model: Option<&str>,
    budget: Option<f64>,
    skip_context: bool,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut ctx = GbaContext::load(&cwd).await?;
    ctx.apply_overrides(model, budget);
    let engine = InitEngine::new(ctx)?;
    let result = engine.run(skip_context).await?;

    if result.is_fresh_init && skip_context {
        println!("GBA initialized: repository analyzed (context generation skipped).");
    } else if result.is_fresh_init {
        println!(
            "GBA initialized: {} context document(s) generated.",
            result.context_doc_count
        );
    } else if skip_context {
        // Already initialized and skip_context: nothing was done.
        println!("Already up-to-date.");
    } else if result.context_doc_count == 0 {
        println!(
            "GBA context is already up-to-date ({} document(s) checked).",
            result.context_docs_skipped
        );
    } else {
        println!(
            "GBA updated: {} context document(s) regenerated, {} unchanged.",
            result.context_doc_count, result.context_docs_skipped
        );
    }

    Ok(())
}
