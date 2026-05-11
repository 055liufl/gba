//! GBA CLI - Command line interface for Geektime Bootcamp Agent.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

use clap::Parser;

/// GBA - Geektime Bootcamp Agent CLI
#[derive(Debug, Parser)]
#[command(name = "gba", version, about, long_about = None)]
struct Cli {
    /// Turn on verbose logging
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tracing::info!("GBA CLI starting...");

    Ok(())
}
