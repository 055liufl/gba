//! Handler for the `gba run` command.
//!
//! Runs the phased implementation for a planned feature, displaying
//! colored progress output via crossterm styled printing.

use std::io::{self, Write};

use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use gba_core::{GbaContext, RunEngine, TaskStatus};
use tracing::info;

/// Execute the run command with colored progress output.
///
/// Loads the project context, creates a `RunEngine`, and executes all
/// remaining tasks for the given feature slug. Each task's progress is
/// printed as a colored checklist line.
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

    print_header(slug)?;

    let engine = RunEngine::new(ctx, slug.to_owned())?;
    let result = engine
        .run(|progress| {
            if let Err(e) = print_progress(&progress) {
                info!(error = %e, "failed to print progress line");
            }
        })
        .await?;

    print_summary(slug, &result)?;

    Ok(())
}

/// Print the run header.
fn print_header(slug: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print(format!("\nRunning feature: {slug}\n")),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("─".repeat(40)),
        Print("\n"),
    )?;
    stdout.flush()
}

/// Print a single progress line with a status icon and colors.
fn print_progress(progress: &gba_core::TaskProgress) -> io::Result<()> {
    let (icon, color) = match progress.status {
        TaskStatus::Completed => ("\u{2713}", Color::Green),
        TaskStatus::Failed => ("\u{2717}", Color::Red),
        TaskStatus::Running => ("\u{2192}", Color::Yellow),
        TaskStatus::Pending => (" ", Color::DarkGrey),
        TaskStatus::Skipped => ("-", Color::DarkGrey),
    };

    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        SetForegroundColor(color),
        Print(format!("  [{icon}] ")),
        SetForegroundColor(Color::White),
        SetAttribute(Attribute::Bold),
        Print(progress.kind_label()),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print(format!(": {}\n", progress.description)),
    )?;
    stdout.flush()
}

/// Print the run summary with totals.
fn print_summary(slug: &str, result: &gba_core::RunResult) -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        Print("\n"),
        Print("─".repeat(40)),
        Print("\n"),
        SetForegroundColor(Color::Green),
        SetAttribute(Attribute::Bold),
    )?;

    if result.pr_url.is_empty() {
        crossterm::execute!(
            stdout,
            Print(format!(
                "Run completed for '{slug}': {} turns, ${:.2}\n",
                result.total_turns, result.total_cost_usd
            )),
        )?;
    } else {
        crossterm::execute!(
            stdout,
            Print(format!("PR created: {}\n", result.pr_url)),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print(format!(
                "Total: {} turns, ${:.2}\n",
                result.total_turns, result.total_cost_usd
            )),
        )?;
    }

    crossterm::execute!(
        stdout,
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("\n"),
    )?;
    stdout.flush()
}
