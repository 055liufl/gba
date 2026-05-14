//! GBA CLI - Command line interface for Geektime Bootcamp Agent.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

mod cli;
mod commands;
mod tui;

use std::{
    io::{self, Write},
    process::ExitCode,
};

use clap::Parser;
use cli::{Cli, Command};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use gba_core::GbaCoreError;

#[tokio::main]
async fn main() -> ExitCode {
    // Install Ctrl+C handler that exits cleanly.
    // The TUI plan command already restores terminal state on drop,
    // but this covers non-TUI commands and unexpected interruptions.
    tokio::spawn(async {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            // Restore terminal state in case we're in raw mode.
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            std::process::exit(130);
        }
    });

    let cli = Cli::parse();

    // Initialize tracing with the appropriate filter level.
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let result = run(cli).await;

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let msg = format_error(&err);
            let _ = print_error(&msg);
            ExitCode::FAILURE
        }
    }
}

/// Dispatch the CLI command, applying global overrides.
async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Init { skip_context } => {
            commands::init::execute(cli.model.as_deref(), cli.budget, skip_context).await
        }
        Command::Plan { slug } => {
            commands::plan::execute(&slug, cli.model.as_deref(), cli.budget).await
        }
        Command::Run { slug } => {
            commands::run::execute(&slug, cli.model.as_deref(), cli.budget).await
        }
        Command::List => commands::list::execute().await,
    }
}

/// Format an error for user-friendly display.
///
/// Checks for known `GbaCoreError` variants and provides actionable messages.
/// Falls back to displaying the full error chain for unknown errors.
fn format_error(err: &anyhow::Error) -> String {
    if let Some(core_err) = err.downcast_ref::<GbaCoreError>() {
        return match core_err {
            GbaCoreError::NotInitialized => "This project hasn't been initialized yet. Run `gba \
                                             init` to get started."
                .to_owned(),
            GbaCoreError::ContextNotFound { .. } => "Could not find a git repository. Make sure \
                                                     you're inside a project directory."
                .to_owned(),
            GbaCoreError::FeatureNotFound { slug } => {
                format!("Feature '{slug}' not found. Run `gba list` to see available features.")
            }
            GbaCoreError::WorktreeNotFound { path } => {
                format!(
                    "Worktree for feature is missing at '{}'. It may have been cleaned up \
                     manually.",
                    path.display()
                )
            }
            GbaCoreError::InvalidSlug { slug } => {
                format!(
                    "Invalid slug '{slug}'. Slugs must be lowercase alphanumeric words separated \
                     by hyphens (e.g., 'web-frontend'), at most 64 characters."
                )
            }
            _ => format_error_chain(err),
        };
    }

    format_error_chain(err)
}

/// Format the full error chain for display.
fn format_error_chain(err: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    parts.push(format!("{err}"));
    let mut source = err.source();
    while let Some(cause) = source {
        parts.push(format!("  caused by: {cause}"));
        source = std::error::Error::source(cause);
    }
    parts.join("\n")
}

/// Print an error message to stderr with red "error:" prefix.
fn print_error(msg: &str) -> io::Result<()> {
    let mut stderr = io::stderr();
    crossterm::execute!(
        stderr,
        SetForegroundColor(Color::Red),
        Print("error: "),
        ResetColor,
        Print(msg),
        Print("\n"),
    )?;
    stderr.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_format_not_initialized_error() {
        let err = anyhow::Error::new(GbaCoreError::NotInitialized);
        let msg = format_error(&err);
        assert!(msg.contains("hasn't been initialized"));
        assert!(msg.contains("gba init"));
    }

    #[test]
    fn test_should_format_context_not_found_error() {
        let err = anyhow::Error::new(GbaCoreError::ContextNotFound {
            start_dir: "/tmp/test".into(),
        });
        let msg = format_error(&err);
        assert!(msg.contains("git repository"));
        assert!(msg.contains("project directory"));
    }

    #[test]
    fn test_should_format_feature_not_found_error() {
        let err = anyhow::Error::new(GbaCoreError::FeatureNotFound {
            slug: "web-frontend".to_owned(),
        });
        let msg = format_error(&err);
        assert!(msg.contains("web-frontend"));
        assert!(msg.contains("gba list"));
    }

    #[test]
    fn test_should_format_worktree_not_found_error() {
        let err = anyhow::Error::new(GbaCoreError::WorktreeNotFound {
            path: "/tmp/.trees/0001_feat".into(),
        });
        let msg = format_error(&err);
        assert!(msg.contains("Worktree"));
        assert!(msg.contains("cleaned up manually"));
    }

    #[test]
    fn test_should_format_generic_error_with_chain() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err = anyhow::Error::new(inner).context("reading config");
        let msg = format_error(&err);
        assert!(msg.contains("reading config"));
        assert!(msg.contains("file gone"));
    }
}
