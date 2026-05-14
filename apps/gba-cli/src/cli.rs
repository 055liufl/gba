//! CLI argument definitions using Clap.

use clap::{Parser, Subcommand};

/// GBA - Geektime Bootcamp Agent CLI.
///
/// An AI-powered tool that adds features to your codebase via
/// multi-phase planning, implementation, review, and PR submission.
#[derive(Debug, Parser)]
#[command(name = "gba", version, about, long_about = None)]
pub struct Cli {
    /// Turn on verbose (debug-level) logging.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override the model to use.
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// Override the maximum budget in USD.
    #[arg(long, global = true)]
    pub budget: Option<f64>,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize GBA in the current repository.
    ///
    /// Analyzes the repo structure and creates the `.gba/` directory
    /// with project context documents. Re-running on an already-initialized
    /// project performs an incremental update, regenerating only context
    /// documents whose directory contents have changed.
    Init {
        /// Skip generating per-directory context documents (`.gba.md`).
        ///
        /// Only the repository analysis is performed. Useful for a quick
        /// first pass or when the LLM context documents are not needed.
        #[arg(long, default_value_t = false)]
        skip_context: bool,
    },

    /// Plan a new feature interactively.
    ///
    /// Opens a multi-turn conversation to design the feature, then
    /// generates specs and an implementation plan.
    Plan {
        /// URL-friendly slug for the feature (e.g., `web-frontend`).
        slug: String,
    },

    /// Run (or resume) the implementation of a planned feature.
    ///
    /// Executes the phased implementation plan: build, commit,
    /// verify, review, fix, and submit PR.
    Run {
        /// Feature slug to run.
        slug: String,
    },

    /// List all features and their statuses.
    List,
}
