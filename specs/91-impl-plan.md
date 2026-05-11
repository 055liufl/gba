# GBA — Implementation Plan

## Phase Overview

```
Phase 0: Foundation        ──>  types, errors, state, prompt manager, agent runner
Phase 1: Init Engine       ──>  gba init (repo analysis, scaffolding)
Phase 2: Plan Engine       ──>  gba plan (multi-turn conversation, spec generation)
Phase 3: Run Engine        ──>  gba run (phased execution, resume, cost tracking, PR)
Phase 4: TUI               ──>  ratatui interactive interface for gba plan
Phase 5: Polish            ──>  list command, error UX, config, edge cases
```

---

## Phase 0 — Foundation

**Goal**: Establish shared types, error handling, feature state persistence, prompt manager, and the agent runner wrapper.

### 0.1 — gba-pm: Prompt Manager

- [ ] Define `PromptManager` struct with MiniJinja `Environment`
- [ ] Embed all templates from `crates/gba-pm/templates/` via `include_str!` at compile time
- [ ] Implement `PromptManager::new()` — initialize MiniJinja env, register all templates
- [ ] Implement `PromptManager::render(name, ctx)` — render template with JSON context
- [ ] Implement `PromptManager::list_templates()` — return registered template names
- [ ] Define `GbaPmError` with `thiserror` (variants: `TemplateNotFound`, `RenderError`)
- [ ] Unit tests: render with valid context, render with missing var, list templates

### 0.2 — gba-core: Types & State

- [ ] Define `TaskKind` enum (`Setup`, `Build`, `Commit`, `Verification`, `Review`, `ReviewFix`, `SubmitPr`)
- [ ] Define `TaskStatus` enum (`Pending`, `Running`, `Completed`, `Failed`, `Skipped`)
- [ ] Define `FeatureStatus` enum (`Planned`, `Running`, `Reviewing`, `Completed`, `Failed`)
- [ ] Define `TaskState` struct (id, kind, description, status, turns, cost_usd, commit_sha, completed_at)
- [ ] Define `PlanState` struct (turns, cost_usd, completed_at)
- [ ] Define `PrState` struct (url, number)
- [ ] Define `TotalsState` struct (turns, cost_usd)
- [ ] Define `FeatureState` struct (feature info, status, plan, tasks vec, totals, pr)
- [ ] Implement `FeatureState::load(path)` — deserialize from `state.yml`
- [ ] Implement `FeatureState::save(&self)` — serialize to `state.yml`
- [ ] Implement `FeatureState::find_resume_point()` — return first non-completed task index
- [ ] Implement `FeatureState::update_task(id, status, turns, cost)` — update and save atomically
- [ ] Define `Feature` struct (number, slug, path)
- [ ] Define result types: `InitResult`, `PlanResult`, `RunResult` (includes total turns, cost, pr_url)
- [ ] Define `TaskProgress` struct for callbacks (task_id, kind, description, status)
- [ ] Define `GbaCoreError` with `thiserror`
- [ ] Unit tests: state load/save roundtrip, resume point finding, task update

### 0.3 — gba-core: TaskPreset

- [ ] Define `TaskPreset` struct (`permission_mode`, `max_turns`, `system_prompt_template`, `user_prompt_template`)
- [ ] Define `PresetKind` enum matching all task scenarios: `InitAnalyze`, `InitGenerateContext`, `PlanConversation`, `PlanGenerateSpec`, `Build`, `BuildResume`, `Commit`, `Verification`, `Review`, `ReviewFix`, `SubmitPr`
- [ ] Implement `PresetKind::preset()` — hard-coded mapping returning `TaskPreset` with correct permission_mode and max_turns
- [ ] Implement `PresetKind::apply_config_override(config)` — apply user config overrides (only tighten max_turns)
- [ ] Unit tests: each preset has correct permission_mode, config override only tightens

### 0.4 — gba-core: GbaContext

- [ ] Define `GbaContext` struct (project root, `.gba` path, `.trees` path, config)
- [ ] Define `GbaConfig` struct (model, max_budget_usd, permission_mode) — deserialized from `config.yml`
- [ ] Implement `GbaContext::load(cwd)` — discover project root (walk up to find `.gba/` or `.git/`)
- [ ] Implement `GbaContext::is_initialized()` — check `.gba/` exists
- [ ] Implement `GbaContext::feature_dir(slug)` — resolve `.gba/<NNNN>_<slug>/`
- [ ] Implement `GbaContext::next_feature_number()` — scan `.gba/` for max N, return N+1
- [ ] Implement `GbaContext::load_config()` — read `.gba/config.yml` with defaults
- [ ] Unit tests: context loading, feature numbering, config defaults

### 0.5 — gba-core: AgentRunner

- [ ] Define `AgentRunner` struct wrapping `claude-agent-sdk-rs`
- [ ] Implement `AgentRunner::new(cwd, config)` — base configuration
- [ ] Implement `AgentRunner::query(preset, prompt)` — one-shot query using `TaskPreset` to configure `ClaudeAgentOptions` (permission_mode, max_turns, system_prompt). Return `AgentResult { text, turns, cost_usd }`
- [ ] Implement `AgentRunner::start_session(preset)` — create `ClaudeClient` for multi-turn with preset's system prompt and permission_mode
- [ ] Implement `AgentRunner::send(message)` — send to existing session, return `AgentResult`
- [ ] Unit tests: construction with options, preset mapping to ClaudeAgentOptions (no live SDK calls)

### 0.6 — gba-cli: CLI Skeleton

- [ ] Define `Cli` struct with subcommands: `Init`, `Plan { slug }`, `Run { slug }`, `List`
- [ ] Wire up `tracing-subscriber` initialization with `--verbose` flag
- [ ] Stub out command handlers that print "not implemented"
- [ ] Verify `cargo run -- init`, `cargo run -- plan test`, `cargo run -- run test`, `cargo run -- list` parse correctly

**Exit criteria**: `cargo build && cargo test && cargo clippy -- -D warnings` all pass. `gba --help` shows subcommands. State load/save roundtrips. Prompt manager renders templates.

---

## Phase 1 — Init Engine

**Goal**: `gba init` analyzes the repo and scaffolds `.gba/` + `.trees/`.

### 1.1 — InitEngine implementation

- [ ] Implement `InitEngine::new(ctx)` with `AgentRunner` + `PromptManager`
- [ ] Implement `InitEngine::run()`:
  1. Check `is_initialized()` → return early if true
  2. Create `.gba/` and `.trees/` directories
  3. Render `init-analyze-repo` prompt with `{project_root, file_tree, existing_claude_md}`
  4. Call `AgentRunner::query()` to analyze repo
  5. Parse JSON analysis result
  6. For each important directory, render `init-generate-context` and query
  7. Write `.gba.md` files in each directory
  8. Append references to `CLAUDE.md`
  9. Add `.trees/` to `.gitignore`
  10. Return `InitResult`

### 1.2 — Wire init command in CLI

- [ ] Connect `InitCmd` to `InitEngine::run()`
- [ ] Display success/skip message with directory count

**Exit criteria**: `gba init` in a real repo creates `.gba/`, `.trees/`, and context docs.

---

## Phase 2 — Plan Engine

**Goal**: `gba plan <slug>` drives multi-turn conversation and generates specs + state.yml.

### 2.1 — PlanEngine implementation

- [ ] Implement `PlanEngine::new(ctx, slug)` — create `AgentRunner` session + `PromptManager`
- [ ] Implement `PlanEngine::start()`:
  1. Render `plan-system` as system prompt
  2. Render `plan-start-conversation` as first user message
  3. Start multi-turn session via `AgentRunner::start_session()`
  4. Send initial prompt, return assistant's first message
- [ ] Implement `PlanEngine::send(user_input)`:
  1. Forward user message to session
  2. Track turns count
  3. Return assistant response
- [ ] Implement `PlanEngine::finalize()`:
  1. Render `plan-generate-spec` prompt
  2. Send to session, parse output into spec files
  3. Create git worktree under `.trees/<NNNN>_<slug>/` (branch from main)
  4. Write spec files to `.gba/<NNNN>_<slug>/specs/`
  5. Render `plan-generate-impl-plan` if impl-plan not included
  6. Parse impl-plan to build task list
  7. Create `state.yml` with status `planned`, task list, plan cost/turns
  8. Return `PlanResult`

### 2.2 — Wire plan command in CLI (basic, no TUI yet)

- [ ] Connect `PlanCmd` to `PlanEngine` with stdin/stdout for now
- [ ] Simple loop: print assistant message, read user input, repeat
- [ ] On `/done` or empty input, call `finalize()`
- [ ] Print plan summary: feature number, spec files created, worktree path

**Exit criteria**: `gba plan my-feature` starts conversation, generates specs, creates worktree, writes state.yml.

---

## Phase 3 — Run Engine

**Goal**: `gba run <slug>` executes the implementation plan with resume support and cost tracking.

### 3.1 — RunEngine implementation

- [ ] Implement `RunEngine::new(ctx, slug)`:
  1. Load `FeatureState` from `.gba/<feat>/state.yml`
  2. Load specs from `.gba/<feat>/specs/`
  3. Validate worktree exists at `.trees/<feat>/`

- [ ] Implement `RunEngine::run(on_progress)`:
  1. Find resume point via `FeatureState::find_resume_point()`
  2. If all tasks completed → return early with message
  3. Update feature status to `running`
  4. For each task from resume point onward:
     ```
     match task.kind {
         Setup => create directories in worktree
         Build => {
             if resuming (task.status == Running or Failed):
                 render "run-build-phase-resume" with existing_changes, error_message
             else:
                 render "run-build-phase"
             query AgentRunner in worktree cwd
             update task: status=completed, turns, cost_usd
             save state.yml
         }
         Commit => {
             render "run-commit"
             query AgentRunner
             record commit_sha in task
             save state.yml
         }
         Verification => {
             render "run-verification"
             query AgentRunner
             update task status
             save state.yml
         }
         Review => {
             render "run-review" with full diff
             query AgentRunner
             save review output for next task
             update task status
             save state.yml
         }
         ReviewFix => {
             render "run-review-fix" with review_findings
             query AgentRunner
             update task status
             save state.yml
         }
         SubmitPr => {
             render "run-submit-pr"
             query AgentRunner
             parse PR URL from output
             update state.yml: pr.url, pr.number, status=completed
             save state.yml
         }
     }
     callback on_progress(TaskProgress) after each task
     ```
  5. Update totals (sum of all task turns/cost)
  6. Return `RunResult { pr_url, total_turns, total_cost_usd }`

### 3.2 — Wire run command in CLI

- [ ] Connect `RunCmd` to `RunEngine::run()`
- [ ] Display progress with checkmarks: `[✓] Phase 1: Build observer (12 turns, $1.23)`
- [ ] On completion, print summary: total turns, total cost, PR URL
- [ ] Handle Ctrl+C: save current state before exit

**Exit criteria**: `gba run my-feature` executes all phases, commits, reviews, verifies, creates PR. Interrupted runs can be resumed. Cost is tracked.

---

## Phase 4 — TUI (ratatui)

**Goal**: Replace stdin/stdout plan interface with a polished TUI.

### 4.1 — TUI framework

- [ ] Implement `App` struct with state machine (Input, Waiting, Display)
- [ ] Implement event loop: crossterm events + async messages from `PlanEngine`
- [ ] Implement `ui.rs` rendering: chat history panel, input area, status bar

### 4.2 — TUI integration

- [ ] Connect `PlanEngine` to TUI app
- [ ] Render assistant messages as markdown-styled text
- [ ] Handle `/done` command in TUI to trigger finalize
- [ ] Graceful exit on Ctrl+C / Esc

### 4.3 — Progress display for `gba run`

- [ ] Add a checklist view for run command using ratatui widgets
- [ ] Show real-time task updates with status, turns, cost
- [ ] Show running totals at the bottom

**Exit criteria**: `gba plan` launches a usable TUI. `gba run` shows real-time progress.

---

## Phase 5 — Polish

**Goal**: Edge cases, UX, and robustness.

### 5.1 — List command

- [ ] Implement `gba list` — scan `.gba/` and display features with state
- [ ] Show: number, slug, status, total turns, total cost, PR URL (if any)
- [ ] Color-code status: planned=yellow, running=blue, completed=green, failed=red

### 5.2 — Error UX

- [ ] Friendly error messages for: not a git repo, not initialized, feature not found, worktree missing
- [ ] Handle network errors with retry suggestion
- [ ] Handle Ctrl+C gracefully in all commands (save state on interrupt)

### 5.3 — Configuration

- [ ] Support `.gba/config.yml` for overrides (model, max_budget_usd, permission_mode)
- [ ] CLI flags: `--model`, `--budget` override config.yml
- [ ] Merge precedence: CLI flags > config.yml > defaults

### 5.4 — Testing

- [ ] Integration tests with mock agent runner
- [ ] Template rendering tests with realistic contexts for all 13 templates
- [ ] State persistence tests: load/save/resume/update
- [ ] CLI argument parsing tests

**Exit criteria**: All edge cases handled. `gba --help` is clear. No panics on bad input. Interrupted runs always resume correctly.

---

## Dependency Graph

```
Phase 0 ─────┬──> Phase 1 (init)
              │
              ├──> Phase 2 (plan) ──> Phase 4 (TUI)
              │
              └──> Phase 3 (run)
                                  ──> Phase 5 (polish)
```

Phases 1, 2, 3 can be developed in parallel after Phase 0 completes. Phase 4 depends on Phase 2. Phase 5 can start after any phase.

---

## Template Checklist

All templates are in `crates/gba-pm/templates/`:

| Template | Type | Purpose | Used By |
|----------|------|---------|---------|
| `init-system.md.j2` | system | Role: architect analyzing repo | Phase 0/1 |
| `init-analyze-repo.md.j2` | user | Analyze repo structure, output JSON | Phase 1 |
| `init-generate-context.md.j2` | user | Generate .gba.md for a directory | Phase 1 |
| `plan-system.md.j2` | system | Role: architect in planning session | Phase 0/2 |
| `plan-start-conversation.md.j2` | user | First user message in planning | Phase 2 |
| `plan-generate-spec.md.j2` | user | Generate design + verification + impl-plan | Phase 2 |
| `plan-generate-impl-plan.md.j2` | user | Generate impl plan from specs (fallback) | Phase 2 |
| `run-system.md.j2` | system | Role: engineer implementing feature | Phase 0/3 |
| `run-build-phase.md.j2` | user | Implement one phase (fresh start) | Phase 3 |
| `run-build-phase-resume.md.j2` | user | Implement one phase (resume) | Phase 3 |
| `run-commit.md.j2` | user | git add + commit for a phase | Phase 3 |
| `run-verification.md.j2` | user | Build, lint, test, verify | Phase 3 |
| `run-review.md.j2` | user | Code review against specs | Phase 3 |
| `run-review-fix.md.j2` | user | Apply review feedback | Phase 3 |
| `run-submit-pr.md.j2` | user | Push branch + create PR | Phase 3 |
