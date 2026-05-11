# GBA (Geektime Bootcamp Agent) — Product Requirements Document

## Overview

GBA is a CLI tool that wraps the Claude Agent SDK, enabling developers to plan, spec, and implement new features for any repository through an interactive, AI-powered workflow.

## Problem Statement

Adding non-trivial features to an existing codebase requires: understanding the repo structure, writing specs, implementing in phases, reviewing, testing, and submitting PRs. This process is manual, error-prone, and time-consuming. GBA automates this pipeline end-to-end.

## Core Workflow

```
gba init  ──>  gba plan <feature-slug>  ──>  gba run <feature-slug>
```

### Command: `gba init`

- Explore the repo structure via Claude Agent SDK
- Create `.gba/` and `.trees/` directories
- Analyze every important directory and generate `.gba.md` context docs
- Add references to these docs in `CLAUDE.md`
- Idempotent: if already initialized, exit gracefully

### Command: `gba plan <feature-slug>`

- Launch a ratatui TUI for interactive conversation with the AI
- User describes the feature, assistant asks clarifying questions
- Iterate until the plan is refined and user approves
- Create a git worktree under `.trees/<feature-slug>` (branched from main)
- Generate spec documents under `.gba/<feature-slug>/specs/`
- End with: "Plan finished. Please call `gba run` to execute"

### Command: `gba run <feature-slug>`

Execute the implementation plan with progress display:

```
$ gba run <feature-slug>
Executing...
[] Generate directory structure
[] Phase 1: Build <component>
[] Commit phase 1
[] Phase 2: Build tests
[] Commit phase 2
[] Codex review
[] Handle review results
[] Verify system
[] Submit PR
```

## Project Directory Structure

```
<repo>/
├── .gba/
│   └── <NNNN>_<feature-slug>/
│       ├── specs/
│       │   ├── design.md
│       │   ├── verification.md
│       │   └── ...
│       └── docs/
│           └── impl_details.md
├── .trees/
│   └── <NNNN>_<feature-slug>/    # git worktree
└── .gitignore                     # includes .trees
```

## Non-functional Requirements

- Single binary (`gba`)
- Minimal startup latency
- Graceful error handling for network failures and CLI interrupts
- Support for any git-managed repository
