{
  "language": "Rust",
  "framework": "tokio + clap + ratatui + claude-agent-sdk-rs",
  "build_system": "cargo (workspace, resolver = 3)",
  "architecture_summary": "GBA is a Cargo workspace CLI tool that wraps the Claude Agent SDK to drive AI-assisted feature development via a three-command workflow (init → plan → run). The workspace is split into two library crates — gba-core (agent execution engine, state machine, context/preset management) and gba-pm (Jinja2-based prompt template manager via minijinja) — consumed by the gba-cli binary which exposes a Tokio-async, Clap-parsed CLI with an optional Ratatui TUI for interactive planning. All inter-crate communication follows an actor/channel model; errors bubble up with thiserror in libraries and anyhow in the application layer.",
  "directories": [
    {
      "path": "apps/gba-cli/src/",
      "description": "CLI binary entry point (main.rs), Clap command definitions (cli.rs), per-command handlers (commands/init.rs, plan.rs, run.rs, list.rs), and Ratatui TUI components (tui/)",
      "importance": "high"
    },
    {
      "path": "apps/gba-cli/tests/",
      "description": "Integration tests for CLI argument parsing (cli_parsing.rs)",
      "importance": "medium"
    },
    {
      "path": "crates/gba-core/src/",
      "description": "Core agent execution engine: context.rs (execution context), runner.rs (agent runner), state.rs (state machine), types.rs (domain types), preset.rs (preset configs), error.rs, and engine/ sub-module (init/plan/run phase implementations)",
      "importance": "high"
    },
    {
      "path": "crates/gba-core/src/engine/",
      "description": "Phase-specific engine logic split into init.rs, plan.rs, and run.rs — each driving Claude Agent SDK calls for the corresponding gba workflow phase",
      "importance": "high"
    },
    {
      "path": "crates/gba-pm/src/",
      "description": "Prompt manager: manager.rs (template loading/rendering), templates.rs (template registry), error.rs — backed by minijinja for Jinja2-style prompt templates",
      "importance": "high"
    },
    {
      "path": "crates/gba-pm/templates/",
      "description": "Jinja2 prompt template files consumed by gba-pm at runtime",
      "importance": "medium"
    },
    {
      "path": "specs/",
      "description": "Project specifications: PRD (01-gba-prd.md), architecture design (02-gba-design.md), phased implementation plan (91-impl-plan.md), improvements backlog (93-improvements-review.md), and architecture diagram (GBA-design-2.png)",
      "importance": "high"
    },
    {
      "path": "docs/research/",
      "description": "Research memos for dependencies and external APIs (e.g., claude-agent-sdk-rs-api.md); consulted before new research is undertaken",
      "importance": "medium"
    },
    {
      "path": ".github/workflows/",
      "description": "CI/CD pipeline (build.yml): fmt check, clippy, cargo-nextest tests, and git-cliff changelog generation on tag push",
      "importance": "medium"
    },
    {
      "path": "Makefile",
      "description": "Top-level automation targets: build, test (nextest), release (cargo-release + git-cliff), update-submodule",
      "importance": "medium"
    },
    {
      "path": "Cargo.toml",
      "description": "Workspace root: declares members (crates/*, apps/*), resolver = 3, and all shared [workspace.dependencies] including pinned versions",
      "importance": "high"
    }
  ],
  "entry_points": [
    "apps/gba-cli/src/main.rs"
  ],
  "conventions": [
    "Rust 2024 edition; toolchain pinned in rust-toolchain.toml",
    "snake_case for functions/variables, PascalCase for types, SCREAMING_SNAKE_CASE for constants",
    "All workspace dependencies declared once in root Cargo.toml [workspace.dependencies] and referenced with .workspace = true in member crates",
    "Unit tests in the same file under #[cfg(test)] mod tests; integration tests in tests/ directory",
    "Test names use test_should_ prefix describing behavior (e.g., test_should_return_error_on_invalid_input)",
    "Error types use thiserror in library crates (gba-core, gba-pm); anyhow used in the CLI application",
    "tracing used for all structured logging — no println!/dbg! in production code",
    "Actor model with message-passing channels for concurrency; no Mutex/RwLock around non-Send/Sync types",
    "No unwrap()/expect() in production code; errors propagated with ? operator",
    "Each gba command has its own handler module under apps/gba-cli/src/commands/",
    "Per-crate .gba.md context files present (apps/gba-cli/src/.gba.md, crates/gba-core/src/.gba.md, crates/gba-pm/src/.gba.md) for AI-assisted development",
    "Specs placed under specs/ named {feature-name}-{type}.md; docs under docs/ — both with index.md maintenance",
    "Makefile targets used for all automation; no standalone shell scripts",
    "YAML format for all runtime configuration; compile-time constants for build-time tuning"
  ]
}