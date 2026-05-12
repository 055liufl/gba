//! Embedded prompt templates for GBA.
//!
//! All templates are embedded at compile time via `include_str!` so the binary
//! has no runtime file dependencies.

/// All template entries as `(name, content)` pairs.
///
/// Template names match the file stem (without the `.md.j2` extension).
pub const TEMPLATES: &[(&str, &str)] = &[
    // System prompts
    (
        "init-system",
        include_str!("../templates/init-system.md.j2"),
    ),
    (
        "plan-system",
        include_str!("../templates/plan-system.md.j2"),
    ),
    ("run-system", include_str!("../templates/run-system.md.j2")),
    // Init user prompts
    (
        "init-analyze-repo",
        include_str!("../templates/init-analyze-repo.md.j2"),
    ),
    (
        "init-generate-context",
        include_str!("../templates/init-generate-context.md.j2"),
    ),
    // Plan user prompts
    (
        "plan-start-conversation",
        include_str!("../templates/plan-start-conversation.md.j2"),
    ),
    (
        "plan-generate-spec",
        include_str!("../templates/plan-generate-spec.md.j2"),
    ),
    (
        "plan-generate-impl-plan",
        include_str!("../templates/plan-generate-impl-plan.md.j2"),
    ),
    // Run user prompts
    (
        "run-build-phase",
        include_str!("../templates/run-build-phase.md.j2"),
    ),
    (
        "run-build-phase-resume",
        include_str!("../templates/run-build-phase-resume.md.j2"),
    ),
    ("run-commit", include_str!("../templates/run-commit.md.j2")),
    (
        "run-verification",
        include_str!("../templates/run-verification.md.j2"),
    ),
    ("run-review", include_str!("../templates/run-review.md.j2")),
    (
        "run-review-fix",
        include_str!("../templates/run-review-fix.md.j2"),
    ),
    (
        "run-submit-pr",
        include_str!("../templates/run-submit-pr.md.j2"),
    ),
];
