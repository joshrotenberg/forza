//! Route — the rule that maps a trigger to a workflow.
//!
//! Routes are the core dispatch mechanism: they define *when* forza acts
//! (trigger) and *what* it does (workflow). Each route handles exactly one
//! type of work.

use serde::{Deserialize, Serialize};

use crate::condition::RouteCondition;
use crate::subject::{Subject, SubjectKind};

/// How a route is triggered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Fires when the subject has a specific GitHub label.
    Label(String),
    /// Fires when the PR state matches a condition.
    Condition(RouteCondition),
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Trigger::Label(l) => write!(f, "label:{l}"),
            Trigger::Condition(c) => write!(f, "condition:{c}"),
        }
    }
}

/// Which PRs are in scope for condition evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Only PRs on branches created by forza (matching the branch pattern prefix).
    #[default]
    ForzaOwned,
    /// All open PRs in the repo.
    All,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scope::ForzaOwned => write!(f, "forza_owned"),
            Scope::All => write!(f, "all"),
        }
    }
}

/// A named rule that maps a trigger to a workflow.
///
/// All fields are public for observability. Routes are loaded from config
/// and remain immutable for the lifetime of the process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// Whether this route handles issues or PRs.
    pub subject_type: SubjectKind,
    /// What triggers this route.
    pub trigger: Trigger,
    /// Which workflow template to execute.
    pub workflow: String,
    /// Which PRs are in scope (condition routes only).
    #[serde(default)]
    pub scope: Scope,
    /// Maximum concurrent runs for this route.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Poll interval in seconds.
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    /// Maximum retries before applying `forza:needs-human`.
    #[serde(default)]
    pub max_retries: Option<usize>,
    /// Override model for this route.
    #[serde(default)]
    pub model: Option<String>,
    /// Override skills for this route.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// Override MCP config for this route.
    #[serde(default)]
    pub mcp_config: Option<String>,
    /// Override validation commands for this route.
    #[serde(default)]
    pub validation_commands: Option<Vec<String>>,
    /// Maximum budget in USD for a single run on this route.
    #[serde(default)]
    pub max_budget_usd: Option<f64>,
}

fn default_concurrency() -> usize {
    1
}

fn default_poll_interval() -> u64 {
    300
}

impl Route {
    /// Whether this is a label-triggered route.
    pub fn is_label_route(&self) -> bool {
        matches!(self.trigger, Trigger::Label(_))
    }

    /// Whether this is a condition-triggered route.
    pub fn is_condition_route(&self) -> bool {
        matches!(self.trigger, Trigger::Condition(_))
    }

    /// Returns the trigger label, if this is a label route.
    pub fn label(&self) -> Option<&str> {
        match &self.trigger {
            Trigger::Label(l) => Some(l),
            Trigger::Condition(_) => None,
        }
    }

    /// Returns the trigger condition, if this is a condition route.
    pub fn condition(&self) -> Option<&RouteCondition> {
        match &self.trigger {
            Trigger::Label(_) => None,
            Trigger::Condition(c) => Some(c),
        }
    }

    /// Check if this route matches the given subject.
    ///
    /// For label routes: checks if the subject has the trigger label.
    /// For condition routes: evaluates the condition against the subject.
    pub fn matches(&self, subject: &Subject) -> bool {
        if subject.kind != self.subject_type {
            return false;
        }
        match &self.trigger {
            Trigger::Label(label) => subject.has_label(label),
            Trigger::Condition(condition) => condition.matches(subject),
        }
    }

    /// Check if a PR branch is in scope for this condition route.
    ///
    /// Only meaningful for condition routes with `Scope::ForzaOwned`.
    /// Returns `true` for label routes (scope doesn't apply).
    pub fn in_scope(&self, branch: &str, branch_prefix: &str) -> bool {
        match (&self.trigger, self.scope) {
            (Trigger::Label(_), _) => true,
            (Trigger::Condition(_), Scope::All) => true,
            (Trigger::Condition(_), Scope::ForzaOwned) => branch.starts_with(branch_prefix),
        }
    }
}

/// A subject matched to its route — the binding that flows through the pipeline.
///
/// Created once during discovery and carried immutably through execution.
/// The route is never re-evaluated after matching.
#[derive(Debug, Clone)]
pub struct MatchedWork {
    /// The issue or PR to process.
    pub subject: Subject,
    /// Name of the matched route (for logging, state, metrics).
    pub route_name: String,
    /// The matched route configuration.
    pub route: Route,
    /// The resolved workflow name.
    pub workflow_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::RouteCondition;
    use crate::subject::SubjectKind;

    fn make_issue_subject(labels: &[&str]) -> Subject {
        Subject {
            kind: SubjectKind::Issue,
            number: 10,
            repo: "owner/repo".into(),
            title: "Test".into(),
            body: String::new(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            html_url: String::new(),
            author: "user".into(),
            branch: "automation/10-test".into(),
            comments: Vec::new(),
            mergeable: None,
            checks_passing: None,
            review_decision: None,
            is_draft: None,
            base_branch: None,
        }
    }

    fn make_pr_subject(
        mergeable: Option<&str>,
        checks_passing: Option<bool>,
        branch: &str,
    ) -> Subject {
        Subject {
            kind: SubjectKind::Pr,
            number: 42,
            repo: "owner/repo".into(),
            title: "Test PR".into(),
            body: String::new(),
            labels: vec![],
            html_url: String::new(),
            author: "user".into(),
            branch: branch.into(),
            comments: Vec::new(),
            mergeable: mergeable.map(String::from),
            checks_passing,
            review_decision: None,
            is_draft: Some(false),
            base_branch: Some("main".into()),
        }
    }

    fn label_route(label: &str) -> Route {
        Route {
            subject_type: SubjectKind::Issue,
            trigger: Trigger::Label(label.into()),
            workflow: "bug".into(),
            scope: Scope::ForzaOwned,
            concurrency: 1,
            poll_interval: 60,
            max_retries: None,
            model: None,
            skills: None,
            mcp_config: None,
            validation_commands: None,
            max_budget_usd: None,
        }
    }

    fn condition_route(condition: RouteCondition) -> Route {
        Route {
            subject_type: SubjectKind::Pr,
            trigger: Trigger::Condition(condition),
            workflow: "pr-fix-ci".into(),
            scope: Scope::ForzaOwned,
            concurrency: 2,
            poll_interval: 60,
            max_retries: Some(3),
            model: None,
            skills: None,
            mcp_config: None,
            validation_commands: None,
            max_budget_usd: None,
        }
    }

    // ── Route matching ──────────────────────────────────────────────

    #[test]
    fn label_route_matches_by_label() {
        let route = label_route("bug");
        let subject = make_issue_subject(&["bug", "forza:ready"]);
        assert!(route.matches(&subject));
    }

    #[test]
    fn label_route_no_match_without_label() {
        let route = label_route("bug");
        let subject = make_issue_subject(&["enhancement"]);
        assert!(!route.matches(&subject));
    }

    #[test]
    fn label_route_no_match_wrong_subject_type() {
        let route = label_route("bug");
        let mut subject = make_issue_subject(&["bug"]);
        subject.kind = SubjectKind::Pr;
        assert!(!route.matches(&subject));
    }

    #[test]
    fn condition_route_matches_pr_state() {
        let route = condition_route(RouteCondition::CiFailing);
        let subject = make_pr_subject(Some("MERGEABLE"), Some(false), "automation/42-fix");
        assert!(route.matches(&subject));
    }

    #[test]
    fn condition_route_no_match_wrong_state() {
        let route = condition_route(RouteCondition::CiFailing);
        let subject = make_pr_subject(Some("MERGEABLE"), Some(true), "automation/42-fix");
        assert!(!route.matches(&subject));
    }

    #[test]
    fn condition_route_no_match_issue() {
        let route = condition_route(RouteCondition::CiFailing);
        let subject = make_issue_subject(&[]);
        assert!(!route.matches(&subject));
    }

    // ── Scope ───────────────────────────────────────────────────────

    #[test]
    fn forza_owned_scope_matches_automation_branch() {
        let route = condition_route(RouteCondition::CiFailing);
        assert!(route.in_scope("automation/42-fix", "automation/"));
    }

    #[test]
    fn forza_owned_scope_rejects_non_automation_branch() {
        let route = condition_route(RouteCondition::CiFailing);
        assert!(!route.in_scope("feature/new-thing", "automation/"));
    }

    #[test]
    fn all_scope_matches_any_branch() {
        let mut route = condition_route(RouteCondition::CiFailing);
        route.scope = Scope::All;
        assert!(route.in_scope("feature/new-thing", "automation/"));
    }

    #[test]
    fn label_route_scope_always_matches() {
        let route = label_route("bug");
        assert!(route.in_scope("anything", "automation/"));
    }

    // ── Trigger helpers ─────────────────────────────────────────────

    #[test]
    fn is_label_route() {
        assert!(label_route("bug").is_label_route());
        assert!(!condition_route(RouteCondition::CiFailing).is_label_route());
    }

    #[test]
    fn is_condition_route() {
        assert!(condition_route(RouteCondition::CiFailing).is_condition_route());
        assert!(!label_route("bug").is_condition_route());
    }

    #[test]
    fn label_accessor() {
        assert_eq!(label_route("bug").label(), Some("bug"));
        assert_eq!(condition_route(RouteCondition::CiFailing).label(), None);
    }

    #[test]
    fn condition_accessor() {
        assert_eq!(
            condition_route(RouteCondition::CiFailing).condition(),
            Some(&RouteCondition::CiFailing)
        );
        assert_eq!(label_route("bug").condition(), None);
    }

    // ── Display ─────────────────────────────────────────────────────

    #[test]
    fn trigger_display() {
        assert_eq!(Trigger::Label("bug".into()).to_string(), "label:bug");
        assert_eq!(
            Trigger::Condition(RouteCondition::CiFailing).to_string(),
            "condition:ci_failing"
        );
    }

    #[test]
    fn scope_display() {
        assert_eq!(Scope::ForzaOwned.to_string(), "forza_owned");
        assert_eq!(Scope::All.to_string(), "all");
    }

    // ── Serialization ───────────────────────────────────────────────

    #[test]
    fn route_serialization_roundtrip() {
        let route = condition_route(RouteCondition::CiGreenNoObjections);
        let json = serde_json::to_string(&route).unwrap();
        let restored: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.workflow, "pr-fix-ci");
        assert_eq!(restored.max_retries, Some(3));
        assert!(restored.is_condition_route());
    }
}
