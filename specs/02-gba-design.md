# GBA — Architecture Design

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        gba-cli                              │
│                     (clap / ratatui)                         │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌────────────┐  ┌──────────┐  │
│  │ InitCmd  │  │ PlanCmd  │  │  RunCmd    │  │ ListCmd  │  │
│  └────┬─────┘  └────┬─────┘  └─────┬──────┘  └────┬─────┘  │
│       │              │              │               │        │
│       │         ┌────┴─────┐        │               │        │
│       │         │ TUI App  │        │               │        │
│       │         │(ratatui) │        │               │        │
│       │         └────┬─────┘        │               │        │
├───────┼──────────────┼──────────────┼───────────────┼────────┤
│       ▼              ▼              ▼               ▼        │
│  ┌──────────────────────────────────────────────────────┐    │
│  │                    gba-core                          │    │
│  │                 (Runtime Engine)                      │    │
│  │                                                      │    │
│  │  ┌────────────┐  ┌────────────┐  ┌───────────────┐   │    │
│  │  │  InitEngine│  │ PlanEngine │  │  RunEngine    │   │    │
│  │  └─────┬──────┘  └─────┬──────┘  └──────┬────────┘   │    │
│  │        │               │                │            │    │
│  │        ▼               ▼                ▼            │    │
│  │  ┌──────────────────────────────────────────────┐    │    │
│  │  │              AgentRunner                     │    │    │
│  │  │   (wraps claude-agent-sdk-rs query/stream)   │    │    │
│  │  └──────────────────┬───────────────────────────┘    │    │
│  │                     │                                │    │
│  │  ┌──────────────────┴───────────────────────────┐    │    │
│  │  │           FeatureState (.yml)                │    │    │
│  │  │   (persistent state for resume & tracking)   │    │    │
│  │  └──────────────────────────────────────────────┘    │    │
│  └──────────────────────────────────────────────────────┘    │
│                        │                                     │
├────────────────────────┼─────────────────────────────────────┤
│  ┌─────────────────────┼──────────────────────────┐          │
│  │                 gba-pm                         │          │
│  │           (Prompt Manager)                     │          │
│  │                                                │          │
│  │  ┌──────────┐  ┌───────────┐  ┌────────────┐  │          │
│  │  │ Registry │  │ Renderer  │  │  Loader    │  │          │
│  │  │(template │  │(minijinja │  │(file/embed)│  │          │
│  │  │  index)  │  │  engine)  │  │            │  │          │
│  │  └──────────┘  └───────────┘  └────────────┘  │          │
│  └────────────────────────────────────────────────┘          │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                  claude-agent-sdk-rs                          │
│              (Claude Code CLI wrapper)                       │
├──────────────────────────────────────────────────────────────┤
│                        tokio                                 │
└──────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

### gba-cli — Command Line Interface

**Role**: User-facing entry point. Parses CLI args, orchestrates TUI, delegates to `gba-core`.

**Public surface**: `main()` only — this is a binary crate.

**Internal modules**:

| Module | Responsibility |
|--------|---------------|
| `cli` | Clap command/subcommand definitions |
| `tui` | Ratatui application for `gba plan` interactive mode |
| `tui::app` | App state machine, event loop |
| `tui::ui` | Widget rendering (chat view, progress bars) |
| `tui::event` | Keyboard/terminal event handling |

**Dependencies**: `gba-core`, `gba-pm`, `clap`, `ratatui`, `crossterm`

### gba-core — Runtime Engine

**Role**: Core execution logic. Each command maps to an engine that builds prompts (via `gba-pm`), invokes Claude Agent SDK, and processes results. Owns the persistent feature state (`.yml`).

**Public Interface**:

```rust
// -- Context --
pub struct GbaContext { .. }

impl GbaContext {
    pub fn load(cwd: PathBuf) -> Result<Self>;
    pub fn is_initialized(&self) -> bool;
    pub fn feature_dir(&self, slug: &str) -> PathBuf;
}

// -- Feature state (persisted as .yml) --
pub struct FeatureState { .. }   // see "Feature State YAML" section

impl FeatureState {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn save(&self) -> Result<()>;
}

// -- Init engine --
pub struct InitEngine { .. }

impl InitEngine {
    pub fn new(ctx: GbaContext) -> Self;
    pub async fn run(&self) -> Result<InitResult>;
}

// -- Plan engine (multi-turn conversational) --
pub struct PlanEngine { .. }

impl PlanEngine {
    pub fn new(ctx: GbaContext, feature_slug: String) -> Self;
    pub async fn start(&mut self) -> Result<AssistantMessage>;
    pub async fn send(&mut self, user_input: &str) -> Result<AssistantMessage>;
    pub async fn finalize(&mut self) -> Result<PlanResult>;
}

// -- Run engine (phased execution with resume) --
pub struct RunEngine { .. }

impl RunEngine {
    pub fn new(ctx: GbaContext, feature_slug: String) -> Self;
    /// Runs (or resumes) phased execution. Reads FeatureState to skip completed tasks.
    pub async fn run(&self, on_progress: impl Fn(TaskProgress)) -> Result<RunResult>;
}
```

**Internal modules**:

| Module | Responsibility |
|--------|---------------|
| `context` | `GbaContext` — project metadata, paths, config |
| `state` | `FeatureState`, `TaskState` — YAML persistence, resume logic |
| `preset` | `TaskPreset` — SDK config per task kind (permission_mode, max_turns, templates) |
| `engine::init` | `InitEngine` — repo analysis, `.gba/` scaffolding |
| `engine::plan` | `PlanEngine` — multi-turn conversation, spec generation |
| `engine::run` | `RunEngine` — phased build, commit, review, verify, PR |
| `runner` | `AgentRunner` — thin wrapper over `claude-agent-sdk-rs`, accepts `TaskPreset` |
| `types` | Shared domain types (`TaskKind`, `Feature`, results) |
| `error` | `GbaCoreError` |

**Dependencies**: `gba-pm`, `claude-agent-sdk-rs`, `tokio`, `serde`, `serde_yml`

### gba-pm — Prompt Manager

**Role**: Load, manage, and render prompt templates. Templates are embedded at compile time and rendered with runtime context using MiniJinja.

**Public Interface**:

```rust
pub struct PromptManager { .. }

impl PromptManager {
    pub fn new() -> Result<Self>;
    pub fn render(&self, name: &str, ctx: &serde_json::Value) -> Result<String>;
    pub fn list_templates(&self) -> Vec<&str>;
}
```

**Internal modules**:

| Module | Responsibility |
|--------|---------------|
| `manager` | `PromptManager` — public entry point |
| `templates` | Embedded template strings (compile-time `include_str!`) |

**Dependencies**: `minijinja`, `serde`, `serde_json`

## Feature State YAML

Each feature has a `state.yml` file at `.gba/<NNNN>_<slug>/state.yml` that tracks the full lifecycle. This file enables **resume on interruption** and **cost tracking**.

### Schema

```yaml
# .gba/0001_web-frontend/state.yml
feature:
  number: 1
  slug: "web-frontend"
  branch: "feat/0001-web-frontend"
  created_at: "2026-05-11T10:30:00Z"

status: "running"  # planned | running | reviewing | completed | failed

plan:
  turns: 8
  cost_usd: 0.42
  completed_at: "2026-05-11T10:35:00Z"

tasks:
  - id: 1
    kind: "setup"
    description: "Generate directory structure"
    status: "completed"    # pending | running | completed | failed | skipped
    turns: 0
    cost_usd: 0.0
    completed_at: "2026-05-11T10:36:00Z"

  - id: 2
    kind: "build"
    description: "Phase 1: Build observer module"
    status: "completed"
    turns: 12
    cost_usd: 1.23
    completed_at: "2026-05-11T10:42:00Z"

  - id: 3
    kind: "commit"
    description: "Commit phase 1"
    status: "completed"
    turns: 0
    cost_usd: 0.0
    commit_sha: "abc1234"
    completed_at: "2026-05-11T10:42:05Z"

  - id: 4
    kind: "build"
    description: "Phase 2: Build event processor"
    status: "running"       # <-- interrupted here, will resume
    turns: 5
    cost_usd: 0.67

  - id: 5
    kind: "commit"
    description: "Commit phase 2"
    status: "pending"

  - id: 6
    kind: "verification"
    description: "Run tests and verify system"
    status: "pending"
    turns: 0
    cost_usd: 0.0

  - id: 7
    kind: "review"
    description: "Code review"
    status: "pending"

  - id: 8
    kind: "review_fix"
    description: "Apply review feedback"
    status: "pending"

  - id: 9
    kind: "verification"
    description: "Post-review verification"
    status: "pending"

  - id: 10
    kind: "submit_pr"
    description: "Submit pull request"
    status: "pending"

totals:
  turns: 25
  cost_usd: 2.32

pr:
  url: ""               # filled after PR is created
  number: 0
```

### TaskKind Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Setup,
    Build,
    Commit,
    Verification,
    Review,
    ReviewFix,
    SubmitPr,
}
```

### TaskPreset

Each task/command maps to a `TaskPreset` that configures the Claude Agent SDK call. This is a **security boundary** — the mapping is hard-coded in the engine, not user-configurable.

```rust
#[derive(Debug, Clone)]
pub struct TaskPreset {
    pub permission_mode: PermissionMode,
    pub max_turns: u32,
    pub system_prompt_template: &'static str,   // template name
    pub user_prompt_template: &'static str,     // template name
}
```

#### Preset Matrix

```
┌────────────────────┬───────────────────┬────────────┬──────────────────────────┐
│ Task               │ permission_mode   │ max_turns  │ Rationale                │
├────────────────────┼───────────────────┼────────────┼──────────────────────────┤
│ init-analyze       │ Plan (read-only)  │ 5          │ Only reads file tree     │
│ init-gen-context   │ Plan (read-only)  │ 5          │ Only reads directory     │
├────────────────────┼───────────────────┼────────────┼──────────────────────────┤
│ plan (multi-turn)  │ Plan (read-only)  │ —          │ Discussion, no writes    │
│ plan-generate-spec │ Plan (read-only)  │ 5          │ Engine writes spec files │
├────────────────────┼───────────────────┼────────────┼──────────────────────────┤
│ build_phase        │ BypassPermissions │ 100        │ Full coding autonomy     │
│ build_phase_resume │ BypassPermissions │ 100        │ Full coding autonomy     │
│ commit             │ AcceptEdits       │ 3          │ Only git add/commit      │
│ verification       │ AcceptEdits       │ 30         │ Run tests, fix issues    │
│ review             │ Plan (read-only)  │ 5          │ MUST be read-only        │
│ review_fix         │ AcceptEdits       │ 30         │ Fix review findings      │
│ submit_pr          │ AcceptEdits       │ 5          │ Only git push + gh pr    │
└────────────────────┴───────────────────┴────────────┴──────────────────────────┘
```

Key constraints:
- **review is Plan (read-only)** — reviewer must not modify code, to preserve separation of concerns. The diff is passed in the prompt; no tools needed.
- **init and plan-generate-spec are Plan** — AI generates text, engine parses output and writes files. AI never touches the file system directly.
- **build/resume are BypassPermissions** — coding tasks need full autonomy to create files, run tests, iterate on compilation errors.
- **commit/submit_pr have low max_turns** — these are mechanical tasks that should complete quickly.

#### Config Override (config.yml)

Users can only **tighten** constraints, never loosen. config.yml supports:

```yaml
# .gba/config.yml
presets:
  build:
    max_turns: 50          # lower than default 100
  verification:
    max_turns: 15          # lower than default 30
```

The engine enforces: `effective_max_turns = min(preset_default, config_override)`. Permission mode is **not overridable** — review is always read-only, build always has full access.

### TaskStatus Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}
```

### Resume Logic

When `gba run <slug>` starts:

1. Load `state.yml`
2. Find the first task with `status != completed && status != skipped`
3. If it was `running` (interrupted), the prompt includes context about partial work
4. Execute from that task onward
5. After each task completes, update `state.yml` immediately (crash-safe)

```
Resume Decision Flow:

  Load state.yml
       │
       ▼
  All tasks completed? ──yes──> "Nothing to do. Already completed."
       │ no
       ▼
  Find first non-completed task
       │
       ├── status: pending ──> Start normally
       │
       ├── status: running ──> Resume: prompt includes
       │                       "previous attempt was interrupted,
       │                        review existing changes and continue"
       │
       └── status: failed  ──> Resume: prompt includes
                                "previous attempt failed,
                                 review errors and retry"
```

## Directory Layout

```
.gba/
├── config.yml                # GBA project config (optional overrides)
└── 0001_web-frontend/
    ├── state.yml             # Feature lifecycle state (resume, cost, PR)
    ├── specs/
    │   ├── design.md         # Architecture / component design
    │   ├── verification.md   # Test & verification plan
    │   └── impl-plan.md      # Phased implementation plan
    └── docs/
        └── impl_details.md   # Implementation notes (generated during run)

.trees/
└── 0001_web-frontend/       # git worktree (branch: feat/0001-web-frontend)

.gitignore                    # includes: .trees/
```

### config.yml Schema

```yaml
# .gba/config.yml
model: "claude-sonnet-4-20250514"
max_budget_usd: 10.0

# Override max_turns per task (can only tighten, never loosen)
presets:
  build:
    max_turns: 50           # default: 100
  verification:
    max_turns: 15           # default: 30
```

## Key Flows

### Flow 1: `gba init`

```
User                CLI                 Core                  PM              Claude SDK
 │                   │                   │                     │                  │
 │  gba init         │                   │                     │                  │
 │──────────────────>│                   │                     │                  │
 │                   │  InitEngine::run()│                     │                  │
 │                   │──────────────────>│                     │                  │
 │                   │                   │  is_initialized()?  │                  │
 │                   │                   │──┐                  │                  │
 │                   │                   │<─┘ yes → return Ok  │                  │
 │                   │                   │                     │                  │
 │                   │                   │  render("init-      │                  │
 │                   │                   │   analyze-repo")    │                  │
 │                   │                   │────────────────────>│                  │
 │                   │                   │   <prompt string>   │                  │
 │                   │                   │<────────────────────│                  │
 │                   │                   │                     │                  │
 │                   │                   │  query(prompt, opts)│                  │
 │                   │                   │────────────────────────────────────────>│
 │                   │                   │        <analysis result>               │
 │                   │                   │<───────────────────────────────────────│
 │                   │                   │                     │                  │
 │                   │                   │  create .gba/ dirs  │                  │
 │                   │                   │  write .gba.md docs │                  │
 │                   │                   │  update CLAUDE.md   │                  │
 │                   │                   │──┐                  │                  │
 │                   │  InitResult       │<─┘                  │                  │
 │  "Initialized"   │<──────────────────│                     │                  │
 │<──────────────────│                   │                     │                  │
```

### Flow 2: `gba plan <feature-slug>`

```
User                CLI/TUI             Core                  PM              Claude SDK
 │                   │                   │                     │                  │
 │  gba plan feat    │                   │                     │                  │
 │──────────────────>│                   │                     │                  │
 │                   │  PlanEngine::new()│                     │                  │
 │                   │──────────────────>│                     │                  │
 │                   │                   │                     │                  │
 │                   │  start()          │                     │                  │
 │                   │──────────────────>│  render("plan-      │                  │
 │                   │                   │   system")          │                  │
 │                   │                   │────────────────────>│                  │
 │                   │                   │  query(prompt)      │                  │
 │                   │                   │────────────────────────────────────────>│
 │                   │                   │  AssistantMessage   │                  │
 │   "Tell me about  │  AssistantMessage │<──────────────────────────────────────│
 │    your feature"  │<──────────────────│                     │                  │
 │<──────────────────│                   │                     │                  │
 │                   │                   │                     │                  │
 │  (multi-turn conversation continues...)                    │                  │
 │                   │                   │                     │                  │
 │  "/done"          │                   │                     │                  │
 │──────────────────>│  finalize()       │                     │                  │
 │                   │──────────────────>│  render("plan-      │                  │
 │                   │                   │   generate-spec")   │                  │
 │                   │                   │────────────────────>│                  │
 │                   │                   │  query → specs      │                  │
 │                   │                   │────────────────────────────────────────>│
 │                   │                   │  create worktree    │                  │
 │                   │                   │  write specs        │                  │
 │                   │                   │  write state.yml    │                  │
 │                   │                   │  (status: planned)  │                  │
 │   "Plan finished. │  PlanResult       │──┐                  │                  │
 │    Call gba run"  │<──────────────────│<─┘                  │                  │
 │<──────────────────│                   │                     │                  │
```

### Flow 3: `gba run <feature-slug>` (with resume)

```
User                CLI                 Core                  PM              Claude SDK
 │                   │                   │                     │                  │
 │  gba run feat     │                   │                     │                  │
 │──────────────────>│                   │                     │                  │
 │                   │  RunEngine::run() │                     │                  │
 │                   │──────────────────>│                     │                  │
 │                   │                   │  load state.yml     │                  │
 │                   │                   │  find resume point  │                  │
 │                   │                   │──┐                  │                  │
 │                   │                   │<─┘                  │                  │
 │                   │                   │                     │                  │
 │  [✓] Setup        │  on_progress(     │  (skip if completed)│                  │
 │<──────────────────│   Setup)          │                     │                  │
 │                   │                   │                     │                  │
 │                   │                   │  for each pending/running task:        │
 │                   │                   │                     │                  │
 │                   │                   │  render prompt      │                  │
 │                   │                   │  (includes resume   │                  │
 │                   │                   │   context if needed)│                  │
 │                   │                   │────────────────────>│                  │
 │                   │                   │  query(prompt, opts)│                  │
 │                   │                   │────────────────────────────────────────>│
 │                   │                   │  result + cost      │                  │
 │  [✓] Phase N      │  on_progress(     │<──────────────────────────────────────│
 │<──────────────────│   Build)          │                     │                  │
 │                   │                   │  update state.yml   │                  │
 │                   │                   │  (turns, cost,      │                  │
 │                   │                   │   status=completed) │                  │
 │                   │                   │──┐                  │                  │
 │                   │                   │<─┘                  │                  │
 │                   │                   │                     │                  │
 │  ... (build → commit → verify → review → verify → PR) ... │                  │
 │                   │                   │                     │                  │
 │                   │                   │  update state.yml   │                  │
 │                   │                   │  (pr.url, totals,   │                  │
 │   RunResult       │  RunResult        │   status=completed) │                  │
 │   (PR link,       │<──────────────────│──┐                  │                  │
 │    total cost)    │                   │<─┘                  │                  │
 │<──────────────────│                   │                     │                  │
```

## Prompt Templates

All templates live in `crates/gba-pm/templates/` and are embedded at compile time.

Templates are divided into **system prompts** (set AI role/constraints via `ClaudeAgentOptions.system_prompt`) and **user prompts** (drive specific tasks via `query(prompt)` or `client.send(prompt)`).

### Template Inventory

```
crates/gba-pm/templates/
│
│ SYSTEM PROMPTS (one per command scope, set via ClaudeAgentOptions.system_prompt)
├── init-system.md.j2               # Role: software architect analyzing a repo
├── plan-system.md.j2               # Role: architect in collaborative planning session
├── run-system.md.j2                # Role: engineer implementing a feature
│
│ USER PROMPTS — init (one-shot query() calls)
├── init-analyze-repo.md.j2         # Task: analyze repo structure, output JSON
├── init-generate-context.md.j2     # Task: generate .gba.md for a directory
│
│ USER PROMPTS — plan (multi-turn client.send() calls)
├── plan-start-conversation.md.j2   # First user message to kick off planning
├── plan-generate-spec.md.j2        # Finalize: generate spec documents
├── plan-generate-impl-plan.md.j2   # Fallback: generate impl plan from specs
│
│ USER PROMPTS — run (one-shot query() calls, each with run-system as system prompt)
├── run-build-phase.md.j2           # Task: implement one phase (fresh start)
├── run-build-phase-resume.md.j2    # Task: implement one phase (resume after interrupt)
├── run-commit.md.j2                # Task: git add + commit for a phase
├── run-verification.md.j2          # Task: build, lint, test, verify
├── run-review.md.j2                # Task: code review against specs
├── run-review-fix.md.j2            # Task: apply review feedback
└── run-submit-pr.md.j2             # Task: push branch + create PR
```

### How Engine Uses Templates

```
┌─────────────┬───────────────────────┬──────────────────────────────────────┐
│ Command     │ SDK Call              │ system_prompt        │ user prompt  │
├─────────────┼───────────────────────┼──────────────────────┼──────────────┤
│ gba init    │ query(prompt, opts)   │ init-system          │ init-analyze │
│             │ query(prompt, opts)   │ init-system          │ init-gen-ctx │
├─────────────┼───────────────────────┼──────────────────────┼──────────────┤
│ gba plan    │ client.connect(opts)  │ plan-system          │              │
│             │ client.send(msg)      │   (persists)         │ plan-start   │
│             │ client.send(msg)      │   (persists)         │ <user input> │
│             │ client.send(msg)      │   (persists)         │ plan-gen-spec│
├─────────────┼───────────────────────┼──────────────────────┼──────────────┤
│ gba run     │ query(prompt, opts)   │ run-system           │ run-build    │
│  (per task) │ query(prompt, opts)   │ run-system           │ run-commit   │
│             │ query(prompt, opts)   │ run-system           │ run-verify   │
│             │ query(prompt, opts)   │ run-system           │ run-review   │
│             │ query(prompt, opts)   │ run-system           │ run-rev-fix  │
│             │ query(prompt, opts)   │ run-system           │ run-submit   │
└─────────────┴───────────────────────┴──────────────────────┴──────────────┘
```

### Template Variables Reference

**System prompts:**

| Template | Variables | Source |
|----------|-----------|--------|
| `init-system` | (none) | static |
| `plan-system` | `project_summary` | init analyze result |
| `run-system` | `feature_slug` | CLI arg |

**User prompts:**

| Template | Variables | Source |
|----------|-----------|--------|
| `init-analyze-repo` | `project_root`, `file_tree`, `claude_md`? | `GbaContext`, fs scan, read file |
| `init-generate-context` | `project_summary`, `dir_path`, `file_list` | analyze result, per-dir scan |
| `plan-start-conversation` | `feature_slug` | CLI arg |
| `plan-generate-spec` | `feature_slug` | CLI arg |
| `plan-generate-impl-plan` | `feature_slug`, `design_spec`, `verification_spec` | spec files |
| `run-build-phase` | `phase_number`, `phase_description`, `design_spec`, `impl_plan`, `verification_spec` | `state.yml` + spec files |
| `run-build-phase-resume` | `phase_number`, `phase_description`, `previous_status`, `design_spec`, `impl_plan`, `verification_spec` | `state.yml` + spec files |
| `run-commit` | `feature_slug`, `phase_number`, `phase_description` | `state.yml` |
| `run-verification` | `feature_slug`, `verification_spec` | spec files |
| `run-review` | `feature_slug`, `design_spec`, `verification_spec`, `all_changes_diff` | spec files + `git diff` |
| `run-review-fix` | `feature_slug`, `review_findings`, `design_spec` | review output + spec files |
| `run-submit-pr` | `feature_slug`, `design_spec` | spec files |

`?` = conditional (included only when non-empty). All other variables are required.
`feature_slug` in run user prompts is provided via `run-system` system prompt, not repeated in each user prompt.

## Code Structure

```
crates/gba-core/src/
├── lib.rs
├── context.rs           # GbaContext
├── state.rs             # FeatureState, TaskState, TaskKind, TaskStatus
├── preset.rs            # TaskPreset, TaskKind → preset mapping
├── engine/
│   ├── mod.rs
│   ├── init.rs          # InitEngine
│   ├── plan.rs          # PlanEngine
│   └── run.rs           # RunEngine
├── runner.rs            # AgentRunner (accepts TaskPreset for SDK config)
├── types.rs             # Feature, InitResult, PlanResult, RunResult, TaskProgress
└── error.rs             # GbaCoreError

crates/gba-pm/src/
├── lib.rs
├── manager.rs           # PromptManager
├── templates.rs         # Embedded template strings via include_str!
└── error.rs             # GbaPmError

crates/gba-pm/templates/
├── init-system.md.j2            # system prompt
├── init-analyze-repo.md.j2      # user prompt
├── init-generate-context.md.j2  # user prompt
├── plan-system.md.j2            # system prompt
├── plan-start-conversation.md.j2
├── plan-generate-spec.md.j2
├── plan-generate-impl-plan.md.j2
├── run-system.md.j2             # system prompt
├── run-build-phase.md.j2
├── run-build-phase-resume.md.j2
├── run-commit.md.j2
├── run-verification.md.j2
├── run-review.md.j2
├── run-review-fix.md.j2
└── run-submit-pr.md.j2

apps/gba-cli/src/
├── main.rs
├── cli.rs               # Clap definitions
├── commands/
│   ├── mod.rs
│   ├── init.rs          # handle init command
│   ├── plan.rs          # handle plan command (launches TUI)
│   ├── run.rs           # handle run command (progress display)
│   └── list.rs          # list features with status
├── tui/
│   ├── mod.rs
│   ├── app.rs           # App state, event loop
│   ├── ui.rs            # Rendering
│   └── event.rs         # Input events
└── error.rs             # CliError
```

## Design Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Embed templates at compile time | Single binary, no runtime file dependencies |
| 2 | MiniJinja for templating | Lightweight, Jinja2-compatible, serde integration |
| 3 | `PlanEngine` is stateful (multi-turn) | Interactive conversation requires maintaining context |
| 4 | Progress via callback `Fn(TaskProgress)` | Decouples core logic from display; CLI can render however it wants |
| 5 | Feature numbering is sequential | Simple, deterministic, avoids naming collisions |
| 6 | Git worktrees for isolation | Each feature gets its own working directory, non-destructive |
| 7 | `AgentRunner` wraps SDK | Single point of SDK configuration; engines don't touch SDK directly |
| 8 | `state.yml` for persistence | Enables resume-on-interrupt, cost tracking, and PR link storage |
| 9 | Separate `build_phase_resume` template | Resume prompt needs different context (existing changes, errors) |
| 10 | `verification` as a distinct task kind | Separates "build" from "verify" — verification runs tests independently |
| 11 | Write `state.yml` after each task | Crash-safe: worst case loses one task of work |
| 12 | YAML for all config/state files | Human-readable, easy to inspect and hand-edit |
| 13 | System/user prompt separation | System prompt sets role+constraints (persistent), user prompt carries task data (per-call). Follows SDK's `system_prompt` + `query(prompt)` model |
| 14 | `TaskPreset` hard-coded in engine | Permission mode per task is a security boundary (review must be read-only). Not user-configurable. Config only allows tightening `max_turns` |
| 15 | Review is `Plan` (read-only) | Reviewer must not modify code — separation of concerns. Diff is passed in prompt, no tools needed |
