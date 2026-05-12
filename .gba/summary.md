# GBA Project Summary

GBA (Geektime Bootcamp Agent) is a Rust CLI tool that orchestrates AI-assisted feature development using the Claude Agent SDK. It automates the full lifecycle: repository analysis, multi-turn planning conversations, spec generation, phased code implementation, verification, code review, and PR submission.

## Architecture

- **Workspace**: Rust 2024 edition, 3 crates (`gba-core`, `gba-pm`, `gba-cli`)
- **Async runtime**: Tokio multi-thread
- **Agent SDK**: `claude-agent-sdk-rs` for Claude interactions
- **Templates**: MiniJinja (15 prompt templates embedded at compile time)
- **State**: YAML-based feature state persistence in `.gba/<NNNN>_<slug>/state.yml`
- **TUI**: ratatui + crossterm for interactive planning

## Key Directories

| Directory | Purpose |
|-----------|---------|
| `crates/gba-core` | Core engine: types, state, context, presets, agent runner, init/plan/run engines |
| `crates/gba-pm` | Prompt manager: MiniJinja template loading and rendering |
| `apps/gba-cli` | CLI binary (`gba`): clap commands, TUI, error formatting |
| `specs/` | PRD, architecture design, implementation plan |
| `vendors/` | Vendored git submodules (claude-agent-sdk-rs) |

## Workflow

1. `gba init` — Scan repo, generate `.gba/summary.md` and per-directory `.gba.md` context files
2. `gba plan <slug>` — Multi-turn conversation → specs + impl plan + git worktree + `state.yml`
3. `gba run <slug>` — Execute tasks (build → commit → verify → review → fix → PR) with resume support
4. `gba list` — Show all features with status, cost, and PR links
