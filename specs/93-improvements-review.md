# Deferred Findings & Improvements

Findings deferred from phase-level code reviews. Each entry includes severity, location, and recommended fix shape.

## Phase 1 — Init Engine

| # | Severity | Location | Finding | Fix Shape |
|---|----------|----------|---------|-----------|
| F4 | P2 | `crates/gba-core/src/engine/init.rs` | `.gba/summary.md` is written during init but not documented in the design spec's directory layout | Add `summary.md` to `specs/02-gba-design.md` directory layout section |
| F7 | P2 | `crates/gba-core/src/engine/init.rs:108` | `InitEngine::new` returns `Result` while design spec declares infallible `-> Self` | Accept the deviation (Result is safer) and update `specs/02-gba-design.md` public interface to reflect `Result<Self, GbaCoreError>` |
| F8 | P1 | `crates/gba-core/src/engine/init.rs` | No integration test exercising `InitEngine::run()` end-to-end with a mock agent runner | Add an integration test in `tests/` that mocks `AgentRunner` (or uses a trait) and verifies all outputs: `.gba/` dir, `.trees/` dir, `.gba.md` files, `CLAUDE.md` update, `.gitignore` |
| F11 | P3 | `crates/gba-core/src/engine/init.rs:461` | `should_skip_entry` skips all hidden files, not just directories. Well-known dotfiles like `.dockerignore` and `.editorconfig` could provide useful context | Consider allowing well-known dotfiles in the scan output |
