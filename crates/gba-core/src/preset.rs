//! Task presets — hard-coded SDK configuration per task kind.
//!
//! Each task maps to a `TaskPreset` that configures the Claude Agent SDK call.
//! This is a security boundary: the mapping is hard-coded in the engine, not
//! user-configurable. Users can only tighten `max_turns`, never loosen, and
//! `permission_mode` is never overridable.

use claude_agent_sdk_rs::PermissionMode;

/// SDK configuration for a specific task kind.
///
/// Contains the permission mode, turn limit, and template names
/// used to build `ClaudeAgentOptions` for a query.
#[derive(Debug, Clone)]
pub struct TaskPreset {
    /// Permission mode for the Claude Agent SDK call.
    pub permission_mode: PermissionMode,
    /// Maximum number of turns allowed for this task.
    pub max_turns: u32,
    /// Name of the system prompt template (rendered via `PromptManager`).
    pub system_prompt_template: &'static str,
    /// Name of the user prompt template (rendered via `PromptManager`).
    pub user_prompt_template: &'static str,
}

/// Identifies a specific task scenario, each mapping to a hard-coded `TaskPreset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    /// Analyze the repository structure (read-only).
    InitAnalyze,
    /// Generate context documents for directories (read-only).
    InitGenerateContext,
    /// Multi-turn planning conversation (read-only).
    PlanConversation,
    /// Generate spec documents from conversation (read-only).
    PlanGenerateSpec,
    /// Generate implementation plan from specs (read-only).
    PlanGenerateImplPlan,
    /// Build (implement) a phase — full coding autonomy.
    Build,
    /// Resume a previously interrupted build phase.
    BuildResume,
    /// Git add and commit after a build phase.
    Commit,
    /// Run tests, lint, and verification checks.
    Verification,
    /// Code review (must be read-only).
    Review,
    /// Apply review feedback.
    ReviewFix,
    /// Push branch and submit a pull request.
    SubmitPr,
}

impl PresetKind {
    /// Return the hard-coded `TaskPreset` for this preset kind.
    pub fn preset(self) -> TaskPreset {
        match self {
            Self::InitAnalyze => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 5,
                system_prompt_template: "init-system",
                user_prompt_template: "init-analyze-repo",
            },
            Self::InitGenerateContext => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 5,
                system_prompt_template: "init-system",
                user_prompt_template: "init-generate-context",
            },
            Self::PlanConversation => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 0, // Unlimited — multi-turn session
                system_prompt_template: "plan-system",
                user_prompt_template: "plan-start-conversation",
            },
            Self::PlanGenerateSpec => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 5,
                system_prompt_template: "plan-system",
                user_prompt_template: "plan-generate-spec",
            },
            Self::PlanGenerateImplPlan => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 5,
                system_prompt_template: "plan-system",
                user_prompt_template: "plan-generate-impl-plan",
            },
            Self::Build => TaskPreset {
                permission_mode: PermissionMode::BypassPermissions,
                max_turns: 100,
                system_prompt_template: "run-system",
                user_prompt_template: "run-build-phase",
            },
            Self::BuildResume => TaskPreset {
                permission_mode: PermissionMode::BypassPermissions,
                max_turns: 100,
                system_prompt_template: "run-system",
                user_prompt_template: "run-build-phase-resume",
            },
            Self::Commit => TaskPreset {
                permission_mode: PermissionMode::AcceptEdits,
                max_turns: 3,
                system_prompt_template: "run-system",
                user_prompt_template: "run-commit",
            },
            Self::Verification => TaskPreset {
                permission_mode: PermissionMode::AcceptEdits,
                max_turns: 30,
                system_prompt_template: "run-system",
                user_prompt_template: "run-verification",
            },
            Self::Review => TaskPreset {
                permission_mode: PermissionMode::Plan,
                max_turns: 5,
                system_prompt_template: "run-system",
                user_prompt_template: "run-review",
            },
            Self::ReviewFix => TaskPreset {
                permission_mode: PermissionMode::AcceptEdits,
                max_turns: 30,
                system_prompt_template: "run-system",
                user_prompt_template: "run-review-fix",
            },
            Self::SubmitPr => TaskPreset {
                permission_mode: PermissionMode::AcceptEdits,
                max_turns: 5,
                system_prompt_template: "run-system",
                user_prompt_template: "run-submit-pr",
            },
        }
    }

    /// Apply a user config override to the preset's `max_turns`.
    ///
    /// The override can only tighten (lower) the turn limit, never loosen it.
    /// A `max_turns_override` of `None` or a value greater than the default
    /// is ignored.
    pub fn apply_config_override(self, max_turns_override: Option<u32>) -> TaskPreset {
        let mut preset = self.preset();
        if let Some(override_turns) = max_turns_override {
            // Only tighten: effective = min(default, override).
            // A preset with max_turns == 0 means unlimited — any override tightens.
            if preset.max_turns == 0 || override_turns < preset.max_turns {
                preset.max_turns = override_turns;
            }
        }
        preset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_have_correct_permission_mode_for_review() {
        let preset = PresetKind::Review.preset();
        assert_eq!(preset.permission_mode, PermissionMode::Plan);
    }

    #[test]
    fn test_should_have_correct_permission_mode_for_build() {
        let preset = PresetKind::Build.preset();
        assert_eq!(preset.permission_mode, PermissionMode::BypassPermissions);
    }

    #[test]
    fn test_should_have_correct_permission_mode_for_commit() {
        let preset = PresetKind::Commit.preset();
        assert_eq!(preset.permission_mode, PermissionMode::AcceptEdits);
    }

    #[test]
    fn test_should_have_correct_max_turns_for_build() {
        let preset = PresetKind::Build.preset();
        assert_eq!(preset.max_turns, 100);
    }

    #[test]
    fn test_should_only_tighten_max_turns_on_override() {
        // Override lower than default: should tighten
        let preset = PresetKind::Build.apply_config_override(Some(50));
        assert_eq!(preset.max_turns, 50);

        // Override higher than default: should not change
        let preset = PresetKind::Build.apply_config_override(Some(200));
        assert_eq!(preset.max_turns, 100);
    }

    #[test]
    fn test_should_not_change_max_turns_on_none_override() {
        let preset = PresetKind::Verification.apply_config_override(None);
        assert_eq!(preset.max_turns, 30);
    }

    #[test]
    fn test_should_tighten_unlimited_preset_on_override() {
        // PlanConversation has max_turns == 0 (unlimited)
        let preset = PresetKind::PlanConversation.apply_config_override(Some(20));
        assert_eq!(preset.max_turns, 20);
    }

    #[test]
    fn test_should_have_correct_templates_for_init_analyze() {
        let preset = PresetKind::InitAnalyze.preset();
        assert_eq!(preset.system_prompt_template, "init-system");
        assert_eq!(preset.user_prompt_template, "init-analyze-repo");
    }

    #[test]
    fn test_should_have_correct_templates_for_build_resume() {
        let preset = PresetKind::BuildResume.preset();
        assert_eq!(preset.system_prompt_template, "run-system");
        assert_eq!(preset.user_prompt_template, "run-build-phase-resume");
    }
}
