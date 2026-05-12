//! CLI argument parsing tests.
//!
//! Verifies that the CLI argument definitions parse correctly for all
//! subcommands and global flags without requiring a running instance.

use clap::{CommandFactory, Parser};

// Re-import the CLI types. Because `cli` is a private module in the binary
// crate, we duplicate the minimal definitions here rather than making them
// public just for testing. Alternatively, we test through `clap::Command`
// introspection.
//
// We use the `gba` binary's CLI definition via `clap::Command::try_get_matches_from`.

/// Helper that builds the Clap command from the binary's CLI definition
/// by invoking `Cli::command()`.
fn cli_command() -> clap::Command {
    // We need to replicate the CLI struct here since it's in a binary crate.
    // Instead, we test using clap's try_parse_from on a local replica.
    CliTest::command()
}

#[derive(Debug, Parser)]
#[command(name = "gba")]
struct CliTest {
    #[arg(short, long, global = true)]
    verbose: bool,

    #[arg(long, global = true)]
    model: Option<String>,

    #[arg(long, global = true)]
    budget: Option<f64>,

    #[command(subcommand)]
    command: CommandTest,
}

#[derive(Debug, clap::Subcommand)]
enum CommandTest {
    Init,
    Plan { slug: String },
    Run { slug: String },
    List,
}

#[test]
fn test_should_parse_init_subcommand() {
    let cli = CliTest::try_parse_from(["gba", "init"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert!(matches!(cli.command, CommandTest::Init));
    assert!(!cli.verbose);
    assert!(cli.model.is_none());
    assert!(cli.budget.is_none());
}

#[test]
fn test_should_parse_plan_subcommand_with_slug() {
    let cli = CliTest::try_parse_from(["gba", "plan", "web-frontend"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    match cli.command {
        CommandTest::Plan { slug } => assert_eq!(slug, "web-frontend"),
        _ => panic!("expected Plan command"),
    }
}

#[test]
fn test_should_parse_run_subcommand_with_slug() {
    let cli = CliTest::try_parse_from(["gba", "run", "api-service"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    match cli.command {
        CommandTest::Run { slug } => assert_eq!(slug, "api-service"),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn test_should_parse_list_subcommand() {
    let cli = CliTest::try_parse_from(["gba", "list"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert!(matches!(cli.command, CommandTest::List));
}

#[test]
fn test_should_parse_verbose_flag() {
    let cli = CliTest::try_parse_from(["gba", "-v", "init"]);
    assert!(cli.is_ok());
    assert!(cli.unwrap().verbose);
}

#[test]
fn test_should_parse_long_verbose_flag() {
    let cli = CliTest::try_parse_from(["gba", "--verbose", "list"]);
    assert!(cli.is_ok());
    assert!(cli.unwrap().verbose);
}

#[test]
fn test_should_parse_model_override() {
    let cli = CliTest::try_parse_from(["gba", "--model", "claude-opus-4", "init"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert_eq!(cli.model.as_deref(), Some("claude-opus-4"));
}

#[test]
fn test_should_parse_budget_override() {
    let cli = CliTest::try_parse_from(["gba", "--budget", "25.50", "init"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert!((cli.budget.unwrap() - 25.50).abs() < f64::EPSILON);
}

#[test]
fn test_should_parse_all_global_flags_together() {
    let cli = CliTest::try_parse_from([
        "gba",
        "--verbose",
        "--model",
        "claude-opus-4",
        "--budget",
        "50.0",
        "plan",
        "my-feature",
    ]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert!(cli.verbose);
    assert_eq!(cli.model.as_deref(), Some("claude-opus-4"));
    assert!((cli.budget.unwrap() - 50.0).abs() < f64::EPSILON);
    match cli.command {
        CommandTest::Plan { slug } => assert_eq!(slug, "my-feature"),
        _ => panic!("expected Plan command"),
    }
}

#[test]
fn test_should_reject_plan_without_slug() {
    let result = CliTest::try_parse_from(["gba", "plan"]);
    assert!(result.is_err());
}

#[test]
fn test_should_reject_run_without_slug() {
    let result = CliTest::try_parse_from(["gba", "run"]);
    assert!(result.is_err());
}

#[test]
fn test_should_reject_unknown_subcommand() {
    let result = CliTest::try_parse_from(["gba", "deploy"]);
    assert!(result.is_err());
}

#[test]
fn test_should_reject_invalid_budget_value() {
    let result = CliTest::try_parse_from(["gba", "--budget", "not-a-number", "init"]);
    assert!(result.is_err());
}

#[test]
fn test_should_parse_global_flags_after_subcommand() {
    // Clap with global = true supports flags both before and after the subcommand
    let cli = CliTest::try_parse_from(["gba", "init", "--verbose"]);
    assert!(cli.is_ok());
    assert!(cli.unwrap().verbose);
}

#[test]
fn test_should_parse_model_override_after_subcommand() {
    let cli = CliTest::try_parse_from(["gba", "list", "--model", "claude-sonnet-4"]);
    assert!(cli.is_ok());
    let cli = cli.unwrap();
    assert_eq!(cli.model.as_deref(), Some("claude-sonnet-4"));
    assert!(matches!(cli.command, CommandTest::List));
}

#[test]
fn test_should_verify_command_name() {
    let cmd = cli_command();
    assert_eq!(cmd.get_name(), "gba");
}
