//! Workflow templates — configurable stage chains per issue type.

use serde::{Deserialize, Serialize};

/// Execution strategy for a workflow template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowMode {
    /// Execute stages in order (default).
    #[default]
    Linear,
    /// On each cycle, evaluate all stage conditions and run the first that passes.
    Reactive,
}

/// A named workflow template defining the stage chain for a type of work.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template name (e.g., "bug", "feature", "research", "chore").
    pub name: String,
    /// Execution strategy. Defaults to `Linear`.
    #[serde(default)]
    pub mode: WorkflowMode,
    /// Ordered stages to execute.
    pub stages: Vec<Stage>,
}

impl Stage {
    /// Create a new stage with defaults.
    pub fn new(kind: StageKind) -> Self {
        Self {
            kind,
            optional: false,
            max_retries: default_retries(),
            timeout_secs: None,
            condition: None,
            agentless: false,
            command: None,
            model: None,
            skills: None,
            mcp_config: None,
        }
    }

    /// Mark this stage as optional.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Make this stage agentless with a shell command.
    pub fn agentless(mut self, command: impl Into<String>) -> Self {
        self.agentless = true;
        self.command = Some(command.into());
        self
    }
}

/// A stage in a workflow — a bounded unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    /// Stage identifier.
    pub kind: StageKind,
    /// Whether this stage is optional (can be skipped).
    #[serde(default)]
    pub optional: bool,
    /// Maximum retries for this stage.
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    /// Timeout in seconds for this stage.
    pub timeout_secs: Option<u64>,
    /// Shell command that gates execution. Exit 0 = run, non-zero = skip.
    #[serde(default)]
    pub condition: Option<String>,
    /// If true, run the command directly instead of invoking an agent.
    /// Requires `command` to be set.
    #[serde(default)]
    pub agentless: bool,
    /// Shell command to run for agentless stages.
    #[serde(default)]
    pub command: Option<String>,
    /// Override model for this stage.
    #[serde(default)]
    pub model: Option<String>,
    /// Override skills for this stage.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// Override MCP config file path for this stage.
    #[serde(default)]
    pub mcp_config: Option<String>,
}

fn default_retries() -> u32 {
    2
}

/// Known stage kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    /// Evaluate issue readiness.
    Triage,
    /// Ask for missing information.
    Clarify,
    /// Create an implementation plan.
    Plan,
    /// Write code changes.
    Implement,
    /// Run tests and validation.
    Test,
    /// Review changes for quality.
    Review,
    /// Create or update a pull request.
    OpenPr,
    /// Address PR review feedback.
    RevisePr,
    /// Fix CI failures.
    FixCi,
    /// Merge the PR.
    Merge,
    /// Produce a research report (comment on issue, no PR).
    Research,
    /// Post a summary comment on the issue.
    Comment,
}

impl StageKind {
    /// Human-readable snake_case name for this stage kind.
    pub fn name(self) -> &'static str {
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
        }
    }
}

/// Built-in workflow templates.
pub fn builtin_templates() -> Vec<WorkflowTemplate> {
    // Merge stages attempt `gh pr merge --auto --squash` and then verify that auto-merge
    // was actually activated by querying the `autoMergeRequest` field via `gh pr view --json`.
    // When a repo has no branch protection with required status checks, `--auto` exits 0 but
    // does NOT enable auto-merge on GitHub. The verification step detects this and falls back
    // to a direct `gh pr merge --squash`. Repos with branch protection get auto-merge queued
    // as normal.
    vec![
        WorkflowTemplate {
            name: "bug".into(),
            stages: vec![
                Stage::new(StageKind::Plan),
                Stage::new(StageKind::Implement),
                Stage::new(StageKind::Test),
                Stage::new(StageKind::Review).optional(),
                Stage::new(StageKind::OpenPr),
                Stage::new(StageKind::Merge)
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "feature".into(),
            stages: vec![
                Stage::new(StageKind::Plan),
                Stage::new(StageKind::Implement),
                Stage::new(StageKind::Test),
                Stage::new(StageKind::Review).optional(),
                Stage::new(StageKind::OpenPr),
                Stage::new(StageKind::Merge)
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "chore".into(),
            stages: vec![
                Stage::new(StageKind::Implement),
                Stage::new(StageKind::Test),
                Stage::new(StageKind::OpenPr),
                Stage::new(StageKind::Merge)
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "research".into(),
            stages: vec![
                Stage::new(StageKind::Research),
                Stage::new(StageKind::Comment),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "pr-maintenance".into(),
            mode: WorkflowMode::Reactive,
            stages: vec![
                // Conflicts first — no point fixing CI on a conflicting branch.
                Stage {
                    condition: Some(
                        "gh pr view --json mergeable --jq '.mergeable' 2>/dev/null \
                         | grep -q CONFLICTING"
                            .into(),
                    ),
                    ..Stage::new(StageKind::RevisePr)
                },
                Stage {
                    condition: Some("! gh pr checks --fail-fast 2>/dev/null".into()),
                    ..Stage::new(StageKind::FixCi)
                },
                Stage {
                    condition: Some(
                        "gh pr view --json reviewDecision --jq '.reviewDecision' 2>/dev/null \
                         | grep -q CHANGES_REQUESTED"
                            .into(),
                    ),
                    ..Stage::new(StageKind::RevisePr)
                },
                Stage {
                    condition: Some(
                        "gh pr checks 2>/dev/null && \
                         ! gh pr view --json reviewDecision --jq '.reviewDecision' 2>/dev/null \
                         | grep -q CHANGES_REQUESTED"
                            .into(),
                    ),
                    ..Stage::new(StageKind::Merge).agentless(
                        "gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash",
                    )
                },
            ],
        },
        WorkflowTemplate {
            name: "pr-review".into(),
            stages: vec![Stage::new(StageKind::Review)],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "pr-fix-ci".into(),
            stages: vec![
                Stage::new(StageKind::FixCi),
                Stage::new(StageKind::Merge)
                    .optional()
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "pr-rebase".into(),
            stages: vec![
                Stage::new(StageKind::RevisePr),
                Stage::new(StageKind::Merge)
                    .optional()
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "pr-fix".into(),
            stages: vec![
                Stage::new(StageKind::RevisePr),
                Stage::new(StageKind::FixCi),
                Stage::new(StageKind::Merge)
                    .optional()
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
        WorkflowTemplate {
            name: "pr-merge".into(),
            stages: vec![
                Stage::new(StageKind::Merge)
                    .agentless("gh pr merge --auto --squash 2>/dev/null; gh pr view --json autoMergeRequest --jq '.autoMergeRequest != null' | grep -q true || gh pr merge --squash"),
            ],
            ..Default::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bug_feature_chore_templates_end_with_merge() {
        for name in &["bug", "feature", "chore"] {
            let template = builtin_templates()
                .into_iter()
                .find(|t| t.name == *name)
                .unwrap_or_else(|| panic!("{name} template must exist"));
            let last = template.stages.last().expect("template must have stages");
            assert_eq!(
                last.kind,
                StageKind::Merge,
                "{name} template should end with Merge"
            );
            assert!(last.agentless, "{name} Merge stage must be agentless");
        }
    }

    #[test]
    fn research_template_has_no_merge_stage() {
        let research = builtin_templates()
            .into_iter()
            .find(|t| t.name == "research")
            .expect("research template must exist");
        assert!(
            !research.stages.iter().any(|s| s.kind == StageKind::Merge),
            "research template should not include a Merge stage"
        );
    }

    #[test]
    fn feature_template_has_no_clarify_stage() {
        let feature = builtin_templates()
            .into_iter()
            .find(|t| t.name == "feature")
            .expect("feature template must exist");
        assert!(
            !feature.stages.iter().any(|s| s.kind == StageKind::Clarify),
            "feature template should not include a Clarify stage by default"
        );
    }

    #[test]
    fn feature_template_starts_with_plan() {
        let feature = builtin_templates()
            .into_iter()
            .find(|t| t.name == "feature")
            .expect("feature template must exist");
        assert_eq!(
            feature.stages[0].kind,
            StageKind::Plan,
            "feature template should start with Plan"
        );
    }

    #[test]
    fn stage_condition_field_defaults_to_none() {
        let stage = Stage::new(StageKind::Review).optional();
        assert!(stage.condition.is_none());
    }

    #[test]
    fn stage_condition_round_trips_via_serde() {
        let mut stage = Stage::new(StageKind::Review).optional();
        stage.condition = Some("test -n \"$RUNNER_ISSUE_BODY\"".into());
        let json = serde_json::to_string(&stage).unwrap();
        let restored: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.condition, stage.condition);
    }

    #[test]
    fn stage_condition_absent_in_json_defaults_to_none() {
        let json = r#"{"kind":"review","optional":true,"max_retries":1}"#;
        let stage: Stage = serde_json::from_str(json).unwrap();
        assert!(stage.condition.is_none());
    }

    #[test]
    fn clarify_injection_before_plan() {
        let mut template = builtin_templates()
            .into_iter()
            .find(|t| t.name == "feature")
            .expect("feature template must exist");

        let clarify_stage = Stage::new(StageKind::Clarify);
        let plan_pos = template
            .stages
            .iter()
            .position(|s| s.kind == StageKind::Plan);
        if let Some(pos) = plan_pos {
            template.stages.insert(pos, clarify_stage);
        } else {
            template.stages.insert(0, clarify_stage);
        }

        assert_eq!(template.stages[0].kind, StageKind::Clarify);
        assert_eq!(template.stages[1].kind, StageKind::Plan);
    }

    #[test]
    fn workflow_mode_default_is_linear() {
        let tmpl = WorkflowTemplate {
            name: "test".to_string(),
            stages: vec![],
            ..Default::default()
        };
        assert_eq!(tmpl.mode, WorkflowMode::Linear);
    }

    #[test]
    fn workflow_mode_serde_round_trip() {
        let tmpl = WorkflowTemplate {
            name: "test".to_string(),
            mode: WorkflowMode::Reactive,
            stages: vec![],
        };
        let json = serde_json::to_string(&tmpl).unwrap();
        let restored: WorkflowTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.mode, WorkflowMode::Reactive);
    }

    #[test]
    fn workflow_mode_absent_in_json_defaults_to_linear() {
        let json = r#"{"name":"test","stages":[]}"#;
        let tmpl: WorkflowTemplate = serde_json::from_str(json).unwrap();
        assert_eq!(tmpl.mode, WorkflowMode::Linear);
    }

    #[test]
    fn pr_maintenance_template_is_reactive() {
        let tmpl = builtin_templates()
            .into_iter()
            .find(|t| t.name == "pr-maintenance")
            .expect("pr-maintenance template must exist");
        assert_eq!(tmpl.mode, WorkflowMode::Reactive);
    }

    #[test]
    fn pr_maintenance_template_has_expected_stages() {
        let tmpl = builtin_templates()
            .into_iter()
            .find(|t| t.name == "pr-maintenance")
            .expect("pr-maintenance template must exist");
        let kinds: Vec<StageKind> = tmpl.stages.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&StageKind::FixCi), "must have FixCi stage");
        assert!(
            kinds.contains(&StageKind::RevisePr),
            "must have RevisePr stage"
        );
        assert!(kinds.contains(&StageKind::Merge), "must have Merge stage");
    }

    #[test]
    fn pr_maintenance_all_stages_have_conditions() {
        let tmpl = builtin_templates()
            .into_iter()
            .find(|t| t.name == "pr-maintenance")
            .expect("pr-maintenance template must exist");
        for stage in &tmpl.stages {
            assert!(
                stage.condition.is_some(),
                "pr-maintenance stage {:?} must have a condition",
                stage.kind
            );
        }
    }

    #[test]
    fn all_merge_stages_verify_auto_merge_activation() {
        let templates = builtin_templates();
        for tmpl in &templates {
            for stage in &tmpl.stages {
                if stage.kind == StageKind::Merge && stage.agentless {
                    let cmd = stage.command.as_deref().unwrap_or("");
                    assert!(
                        cmd.contains("autoMergeRequest"),
                        "template '{}' Merge stage command must verify autoMergeRequest, got: {}",
                        tmpl.name,
                        cmd
                    );
                }
            }
        }
    }
}
