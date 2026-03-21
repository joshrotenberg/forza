//! Workflow templates — configurable stage chains per issue type.

use serde::{Deserialize, Serialize};

/// A named workflow template defining the stage chain for a type of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template name (e.g., "bug", "feature", "research", "chore").
    pub name: String,
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

/// Select a workflow template for an issue based on labels and policy.
///
/// Resolution order:
/// 1. For each issue label, check `policy.workflows` for a template name; if
///    found, resolve it against custom then built-in templates.
/// 2. Fall back to `policy.default_workflow` (if set) or `"feature"`, again
///    resolved against custom then built-in templates.
pub fn select_workflow(labels: &[String], policy: &crate::policy::RepoPolicy) -> WorkflowTemplate {
    // Merge custom and built-in templates; custom templates shadow built-ins.
    let all: Vec<WorkflowTemplate> = {
        let mut v = policy.workflow_templates.clone();
        for builtin in builtin_templates() {
            if !v.iter().any(|t| t.name == builtin.name) {
                v.push(builtin);
            }
        }
        v
    };

    // Check policy workflow label mappings first.
    for label in labels {
        if let Some(template_name) = policy.workflows.get(label)
            && let Some(template) = all.iter().find(|t| t.name == *template_name)
        {
            return template.clone();
        }
    }

    // Fall back to configured default or "feature".
    let default_name = policy.default_workflow.as_deref().unwrap_or("feature");
    all.iter()
        .find(|t| t.name == default_name)
        .cloned()
        .unwrap_or_else(|| {
            builtin_templates()
                .into_iter()
                .find(|t| t.name == "feature")
                .unwrap()
        })
}

/// Built-in workflow templates.
pub fn builtin_templates() -> Vec<WorkflowTemplate> {
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
                    .agentless("gh pr checks --watch && gh pr merge --squash --delete-branch"),
            ],
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
                    .agentless("gh pr checks --watch && gh pr merge --squash --delete-branch"),
            ],
        },
        WorkflowTemplate {
            name: "chore".into(),
            stages: vec![
                Stage::new(StageKind::Implement),
                Stage::new(StageKind::Test),
                Stage::new(StageKind::OpenPr),
                Stage::new(StageKind::Merge)
                    .agentless("gh pr checks --watch && gh pr merge --squash --delete-branch"),
            ],
        },
        WorkflowTemplate {
            name: "research".into(),
            stages: vec![
                Stage::new(StageKind::Research),
                Stage::new(StageKind::Comment),
            ],
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

    fn make_policy(
        workflows: std::collections::HashMap<String, String>,
        default_workflow: Option<String>,
        workflow_templates: Vec<WorkflowTemplate>,
    ) -> crate::policy::RepoPolicy {
        crate::policy::RepoPolicy {
            repo: "owner/repo".to_string(),
            eligible_labels: vec![],
            exclude_labels: vec![],
            workflows,
            branch_pattern: "automation/{issue}-{slug}".to_string(),
            max_concurrency: 3,
            concurrency: Default::default(),
            auto_merge: false,
            agent: "claude".to_string(),
            model: None,
            validation_commands: vec![],
            stage_prompts: Default::default(),
            default_workflow,
            workflow_templates,
            skills: vec![],
            mcp_config: None,
        }
    }

    fn make_issue(labels: Vec<String>) -> crate::github::IssueCandidate {
        crate::github::IssueCandidate {
            number: 1,
            repo: "owner/repo".to_string(),
            title: "some issue".to_string(),
            body: String::new(),
            labels,
            state: "open".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            comments: vec![],
        }
    }

    #[test]
    fn label_mapping_selects_bug_workflow() {
        let mut workflows = std::collections::HashMap::new();
        workflows.insert("bug".to_string(), "bug".to_string());
        let policy = make_policy(workflows, None, vec![]);
        let issue = make_issue(vec!["bug".to_string()]);
        let tmpl = select_workflow(&issue.labels, &policy);
        assert_eq!(tmpl.name, "bug");
    }

    #[test]
    fn no_label_match_falls_back_to_default_workflow() {
        let policy = make_policy(Default::default(), Some("chore".to_string()), vec![]);
        let issue = make_issue(vec![]);
        let tmpl = select_workflow(&issue.labels, &policy);
        assert_eq!(tmpl.name, "chore");
    }

    #[test]
    fn no_label_no_default_falls_back_to_feature() {
        let policy = make_policy(Default::default(), None, vec![]);
        let issue = make_issue(vec![]);
        let tmpl = select_workflow(&issue.labels, &policy);
        assert_eq!(tmpl.name, "feature");
    }

    #[test]
    fn title_prefix_does_not_affect_selection() {
        // Issues titled like "fix: ..." should still get "feature" without a label mapping.
        let policy = make_policy(Default::default(), None, vec![]);
        let mut issue = make_issue(vec![]);
        issue.title = "fix: some bug".to_string();
        let tmpl = select_workflow(&issue.labels, &policy);
        assert_eq!(tmpl.name, "feature");
    }

    #[test]
    fn custom_template_shadows_builtin() {
        let custom = WorkflowTemplate {
            name: "bug".to_string(),
            stages: vec![Stage::new(StageKind::Comment)],
        };
        let mut workflows = std::collections::HashMap::new();
        workflows.insert("bug".to_string(), "bug".to_string());
        let policy = make_policy(workflows, None, vec![custom]);
        let issue = make_issue(vec!["bug".to_string()]);
        let tmpl = select_workflow(&issue.labels, &policy);
        assert_eq!(tmpl.name, "bug");
        assert_eq!(tmpl.stages.len(), 1);
        assert_eq!(tmpl.stages[0].kind, StageKind::Comment);
    }
}
