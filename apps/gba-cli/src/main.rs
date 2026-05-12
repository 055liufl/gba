//! GBA CLI - Command line interface for Geektime Bootcamp Agent.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

mod cli;
mod commands;
mod tui;

use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing with the appropriate filter level.
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Command::Init => commands::init::execute().await,
        Command::Plan { slug } => commands::plan::execute(&slug).await,
        Command::Run { slug } => commands::run::execute(&slug).await,
        Command::List => commands::list::execute().await,
    }
}
