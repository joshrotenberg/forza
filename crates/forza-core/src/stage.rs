//! Stage and workflow types.
//!
//! A [`Workflow`] is an ordered list of [`Stage`]s. Workflows are always linear —
//! stages execute in order, stopping on non-optional failure. There is no reactive
//! or branching mode; complexity belongs in the route configuration, not inside
//! workflows.

use serde::{Deserialize, Serialize};

/// The kind of work a stage performs.
///
/// Each variant maps to a prompt template and defines the stage's role in the
/// pipeline. The set is fixed — adding a new kind requires a new prompt template
/// and planner support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    /// Classify the issue and determine priority/routing.
    Triage,
    /// Ask clarifying questions before implementation.
    Clarify,
    /// Analyze the issue, design a solution, produce a plan breadcrumb.
    Plan,
    /// Write code to solve the issue or address the PR.
    Implement,
    /// Run tests and verify the implementation.
    Test,
    /// Review the changes for correctness and style.
    Review,
    /// Create or update a pull request.
    OpenPr,
    /// Rebase and/or address review feedback on an existing PR.
    RevisePr,
    /// Fix CI failures on a PR.
    FixCi,
    /// Merge the PR.
    Merge,
    /// Investigate a topic and write findings (no code changes).
    Research,
    /// Post a comment on the issue/PR with findings or status.
    Comment,
    /// Create an early draft PR for visibility (agentless).
    DraftPr,
}

impl StageKind {
    /// Returns the canonical string name for this stage kind.
    ///
    /// Used in breadcrumb paths, log fields, hook keys, and serialization.
    pub fn name(&self) -> &'static str {
        match self {
            StageKind::Triage => "triage",
            StageKind::Clarify => "clarify",
            StageKind::Plan => "plan",
            StageKind::Implement => "implement",
            StageKind::Test => "test",
            StageKind::Review => "review",
            StageKind::OpenPr => "open_pr",
            StageKind::RevisePr => "revise_pr",
            StageKind::FixCi => "fix_ci",
            StageKind::Merge => "merge",
            StageKind::Research => "research",
            StageKind::Comment => "comment",
            StageKind::DraftPr => "draft_pr",
        }
    }

    /// All stage kinds, in a natural pipeline order.
    pub fn all() -> &'static [StageKind] {
        &[
            StageKind::Triage,
            StageKind::Clarify,
            StageKind::Plan,
            StageKind::Implement,
            StageKind::Test,
            StageKind::Review,
            StageKind::OpenPr,
            StageKind::RevisePr,
            StageKind::FixCi,
            StageKind::Merge,
            StageKind::Research,
            StageKind::Comment,
            StageKind::DraftPr,
        ]
    }
}

impl std::fmt::Display for StageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// How a stage is executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Execution {
    /// Invoke an agent (e.g., Claude) with a generated prompt.
    Agent,
    /// Run a shell command directly — no agent invocation.
    Shell {
        /// The shell command to execute via `sh -c`.
        command: String,
    },
}

impl std::fmt::Display for Execution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Execution::Agent => f.write_str("agent"),
            Execution::Shell { .. } => f.write_str("shell"),
        }
    }
}

/// A single stage in a workflow.
///
/// Stages are the atomic units of work in forza. Each stage either invokes
/// an agent or runs a shell command, and can be gated by a condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    /// What kind of work this stage performs.
    pub kind: StageKind,
    /// How to execute: agent invocation or shell command.
    #[serde(flatten)]
    pub execution: Execution,
    /// If true, failure doesn't stop the workflow.
    #[serde(default)]
    pub optional: bool,
    /// Shell command that gates execution. Exit 0 = run the stage.
    #[serde(default)]
    pub condition: Option<String>,
    /// Override model for this stage.
    #[serde(default)]
    pub model: Option<String>,
    /// Override skills for this stage.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// Override MCP config for this stage.
    #[serde(default)]
    pub mcp_config: Option<String>,
}

impl Stage {
    /// Create a new agent-executed stage.
    pub fn agent(kind: StageKind) -> Self {
        Self {
            kind,
            execution: Execution::Agent,
            optional: false,
            condition: None,
            model: None,
            skills: None,
            mcp_config: None,
        }
    }

    /// Create a new shell-executed (agentless) stage.
    pub fn shell(kind: StageKind, command: impl Into<String>) -> Self {
        Self {
            kind,
            execution: Execution::Shell {
                command: command.into(),
            },
            optional: false,
            condition: None,
            model: None,
            skills: None,
            mcp_config: None,
        }
    }

    /// Mark this stage as optional (failure won't stop the workflow).
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Add a condition that must pass (exit 0) for this stage to run.
    pub fn condition(mut self, cmd: impl Into<String>) -> Self {
        self.condition = Some(cmd.into());
        self
    }

    /// Whether this stage runs a shell command directly.
    pub fn is_agentless(&self) -> bool {
        matches!(self.execution, Execution::Shell { .. })
    }

    /// Returns the shell command if this is an agentless stage.
    pub fn shell_command(&self) -> Option<&str> {
        match &self.execution {
            Execution::Shell { command } => Some(command),
            Execution::Agent => None,
        }
    }
}

/// A named sequence of stages — the blueprint for processing a subject.
///
/// Workflows are always linear: stages execute in order. If a non-optional
/// stage fails, the workflow stops. The poll loop handles multi-action
/// scenarios naturally (fix CI this cycle, merge next cycle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique name referenced by routes.
    pub name: String,
    /// Ordered list of stages.
    pub stages: Vec<Stage>,
    /// Whether this workflow needs a git worktree. Defaults to `true`.
    /// Set to `false` for workflows that only run shell commands (e.g., merge-only).
    #[serde(default = "default_true")]
    pub needs_worktree: bool,
}

fn default_true() -> bool {
    true
}

/// Shell command for the DraftPr stage.
/// Loaded from `commands/draft_pr.sh` at compile time.
/// Available for custom workflows that include a draft_pr stage.
#[allow(dead_code)]
const DRAFT_PR_COMMAND: &str = include_str!("commands/draft_pr.sh");

/// Shell command for the Merge stage.
/// Waits for CI checks to pass before merging.
/// Loaded from `commands/merge.sh` at compile time.
const MERGE_COMMAND: &str = include_str!("commands/merge.sh");

impl Workflow {
    /// Create a new workflow with the given name and stages.
    pub fn new(name: impl Into<String>, stages: Vec<Stage>) -> Self {
        Self {
            name: name.into(),
            stages,
            needs_worktree: true,
        }
    }

    /// Create a workflow that doesn't need a worktree.
    pub fn without_worktree(mut self) -> Self {
        self.needs_worktree = false;
        self
    }

    /// Returns the builtin workflow templates.
    /// Resolve a workflow alias to its canonical name.
    ///
    /// Legacy names (`bug`, `chore`) map to their modern equivalents.
    pub fn resolve_alias(name: &str) -> &str {
        match name {
            "bug" | "chore" => "quick",
            other => other,
        }
    }

    pub fn builtins() -> Vec<Workflow> {
        vec![
            // ── Issue workflows ──────────────────────────────────────
            Workflow::new(
                "quick",
                vec![
                    Stage::agent(StageKind::Implement),
                    Stage::agent(StageKind::Test),
                    Stage::agent(StageKind::OpenPr),
                ],
            ),
            Workflow::new(
                "feature",
                vec![
                    Stage::agent(StageKind::Plan),
                    Stage::agent(StageKind::Implement),
                    Stage::agent(StageKind::Test),
                    Stage::agent(StageKind::Review),
                    Stage::agent(StageKind::OpenPr),
                ],
            ),
            Workflow::new(
                "research",
                vec![
                    Stage::agent(StageKind::Research),
                    Stage::agent(StageKind::Comment),
                ],
            ),
            // ── PR workflows ─────────────────────────────────────────
            Workflow::new(
                "pr-fix",
                vec![
                    Stage::agent(StageKind::RevisePr),
                    Stage::agent(StageKind::FixCi),
                ],
            ),
            Workflow::new("pr-fix-ci", vec![Stage::agent(StageKind::FixCi)]),
            Workflow::new("pr-rebase", vec![Stage::agent(StageKind::RevisePr)]),
            Workflow::new(
                "pr-merge",
                vec![Stage::shell(StageKind::Merge, MERGE_COMMAND)],
            )
            .without_worktree(),
            Workflow::new("pr-review", vec![Stage::agent(StageKind::Review)]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_kind_names_are_stable() {
        assert_eq!(StageKind::Plan.name(), "plan");
        assert_eq!(StageKind::OpenPr.name(), "open_pr");
        assert_eq!(StageKind::FixCi.name(), "fix_ci");
        assert_eq!(StageKind::RevisePr.name(), "revise_pr");
    }

    #[test]
    fn stage_kind_all_has_every_variant() {
        assert_eq!(StageKind::all().len(), 13);
    }

    #[test]
    fn stage_agent_is_not_agentless() {
        let s = Stage::agent(StageKind::Plan);
        assert!(!s.is_agentless());
        assert!(s.shell_command().is_none());
    }

    #[test]
    fn stage_shell_is_agentless() {
        let s = Stage::shell(StageKind::Merge, "gh pr merge");
        assert!(s.is_agentless());
        assert_eq!(s.shell_command(), Some("gh pr merge"));
    }

    #[test]
    fn stage_optional_builder() {
        let s = Stage::agent(StageKind::Test).optional();
        assert!(s.optional);
    }

    #[test]
    fn stage_condition_builder() {
        let s = Stage::agent(StageKind::Test).condition("cargo test --lib");
        assert_eq!(s.condition.as_deref(), Some("cargo test --lib"));
    }

    #[test]
    fn workflow_new_defaults_needs_worktree() {
        let wf = Workflow::new("test", vec![]);
        assert!(wf.needs_worktree);
    }

    #[test]
    fn workflow_without_worktree() {
        let wf = Workflow::new("merge", vec![]).without_worktree();
        assert!(!wf.needs_worktree);
    }

    #[test]
    fn builtins_cover_expected_names() {
        let builtins = Workflow::builtins();
        let names: Vec<&str> = builtins.iter().map(|w| w.name.as_str()).collect();
        assert!(names.contains(&"quick"));
        assert!(names.contains(&"feature"));
        assert!(names.contains(&"research"));
        assert!(names.contains(&"pr-fix"));
        assert!(names.contains(&"pr-fix-ci"));
        assert!(names.contains(&"pr-rebase"));
        assert!(names.contains(&"pr-merge"));
        assert!(names.contains(&"pr-review"));
    }

    #[test]
    fn aliases_resolve_to_quick() {
        assert_eq!(Workflow::resolve_alias("bug"), "quick");
        assert_eq!(Workflow::resolve_alias("chore"), "quick");
        assert_eq!(Workflow::resolve_alias("feature"), "feature");
        assert_eq!(Workflow::resolve_alias("research"), "research");
        assert_eq!(Workflow::resolve_alias("unknown"), "unknown");
    }

    #[test]
    fn pr_merge_does_not_need_worktree() {
        let merge = Workflow::builtins()
            .into_iter()
            .find(|w| w.name == "pr-merge")
            .unwrap();
        assert!(!merge.needs_worktree);
    }

    #[test]
    fn all_merge_stages_use_env_var() {
        for wf in Workflow::builtins() {
            for stage in &wf.stages {
                if stage.kind == StageKind::Merge
                    && let Some(cmd) = stage.shell_command()
                {
                    assert!(
                        cmd.contains("$FORZA_PR_NUMBER"),
                        "merge command in workflow '{}' must use $FORZA_PR_NUMBER: {cmd}",
                        wf.name
                    );
                }
            }
        }
    }

    #[test]
    fn stage_serialization_roundtrip() {
        let stage = Stage::shell(StageKind::Merge, "gh pr merge").optional();
        let json = serde_json::to_string(&stage).unwrap();
        let restored: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.kind, StageKind::Merge);
        assert!(restored.optional);
        assert!(restored.is_agentless());
    }

    #[test]
    fn execution_display() {
        assert_eq!(Execution::Agent.to_string(), "agent");
        assert_eq!(
            Execution::Shell {
                command: "cargo fmt".into()
            }
            .to_string(),
            "shell"
        );
    }

    #[test]
    fn workflow_serialization_roundtrip() {
        let wf = Workflow::new("test", vec![Stage::agent(StageKind::Plan)]);
        let json = serde_json::to_string(&wf).unwrap();
        let restored: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test");
        assert_eq!(restored.stages.len(), 1);
        assert!(restored.needs_worktree);
    }
}
