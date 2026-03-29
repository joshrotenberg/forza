//! Subject — the thing being worked on.
//!
//! Unifies GitHub issues and pull requests into a single type that flows
//! through the entire pipeline. All fields are public and serializable
//! for full observability through REST/MCP/metrics.

use serde::{Deserialize, Serialize};

/// Whether the subject is an issue or a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    /// A GitHub issue to be processed into a PR.
    Issue,
    /// An existing GitHub pull request to be maintained.
    Pr,
}

impl SubjectKind {
    /// Returns `"issue"` or `"pr"` — used for logging, env vars, and display.
    pub fn as_str(&self) -> &'static str {
        match self {
            SubjectKind::Issue => "issue",
            SubjectKind::Pr => "pr",
        }
    }
}

impl std::fmt::Display for SubjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A GitHub issue or pull request to be processed by a workflow.
///
/// Created during the discovery phase and carried immutably through the
/// entire pipeline. All fields are public for full observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    /// Issue or PR.
    pub kind: SubjectKind,
    /// GitHub issue/PR number.
    pub number: u64,
    /// Repository in `owner/name` format.
    pub repo: String,
    /// Issue/PR title.
    pub title: String,
    /// Issue/PR body text.
    pub body: String,
    /// Labels currently applied.
    pub labels: Vec<String>,
    /// HTML URL for linking.
    pub html_url: String,
    /// Author login.
    pub author: String,
    /// Branch name — PR head branch, or generated from pattern for issues.
    pub branch: String,

    // ── PR-specific fields (None for issues) ──────────────────────────
    /// GitHub mergeability state: `"MERGEABLE"`, `"CONFLICTING"`, `"UNKNOWN"`.
    pub mergeable: Option<String>,
    /// Whether CI checks are passing. `None` if checks are still running.
    pub checks_passing: Option<bool>,
    /// Review decision: `"APPROVED"`, `"CHANGES_REQUESTED"`, or `None`.
    pub review_decision: Option<String>,
    /// Whether the PR is a draft.
    pub is_draft: Option<bool>,
    /// Base branch the PR targets (e.g., `"main"`).
    pub base_branch: Option<String>,
}

impl Subject {
    /// The environment variable name for the subject number.
    ///
    /// Returns `"FORZA_ISSUE_NUMBER"` for issues, `"FORZA_PR_NUMBER"` for PRs.
    pub fn number_env_var(&self) -> &'static str {
        match self.kind {
            SubjectKind::Issue => "FORZA_ISSUE_NUMBER",
            SubjectKind::Pr => "FORZA_PR_NUMBER",
        }
    }

    /// Build the full set of environment variables for shell commands.
    ///
    /// These are set on every shell invocation (agentless stages, conditions,
    /// hooks, validation commands) so that commands can reference the subject
    /// without hardcoding.
    pub fn env_vars(
        &self,
        run_id: &str,
        route: &str,
        workflow: &str,
    ) -> Vec<(&'static str, String)> {
        let mut vars = vec![
            ("FORZA_REPO", self.repo.clone()),
            ("FORZA_SUBJECT_TYPE", self.kind.as_str().to_string()),
            ("FORZA_SUBJECT_NUMBER", self.number.to_string()),
            ("FORZA_SUBJECT_TITLE", self.title.clone()),
            ("FORZA_BRANCH", self.branch.clone()),
            ("FORZA_RUN_ID", run_id.to_string()),
            ("FORZA_ROUTE", route.to_string()),
            ("FORZA_WORKFLOW", workflow.to_string()),
            (self.number_env_var(), self.number.to_string()),
        ];

        if let Some(base) = &self.base_branch {
            vars.push(("FORZA_BASE_BRANCH", base.clone()));
        }

        vars
    }

    /// Whether this subject has a specific label.
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue() -> Subject {
        Subject {
            kind: SubjectKind::Issue,
            number: 42,
            repo: "owner/repo".into(),
            title: "Fix the bug".into(),
            body: "It's broken".into(),
            labels: vec!["bug".into(), "forza:ready".into()],
            html_url: "https://github.com/owner/repo/issues/42".into(),
            author: "user".into(),
            branch: "automation/42-fix-the-bug".into(),
            mergeable: None,
            checks_passing: None,
            review_decision: None,
            is_draft: None,
            base_branch: None,
        }
    }

    fn make_pr() -> Subject {
        Subject {
            kind: SubjectKind::Pr,
            number: 99,
            repo: "owner/repo".into(),
            title: "Fix: the bug".into(),
            body: "Fixes #42".into(),
            labels: vec![],
            html_url: "https://github.com/owner/repo/pull/99".into(),
            author: "user".into(),
            branch: "automation/42-fix-the-bug".into(),
            mergeable: Some("MERGEABLE".into()),
            checks_passing: Some(true),
            review_decision: None,
            is_draft: Some(false),
            base_branch: Some("main".into()),
        }
    }

    #[test]
    fn subject_kind_display() {
        assert_eq!(SubjectKind::Issue.as_str(), "issue");
        assert_eq!(SubjectKind::Pr.as_str(), "pr");
        assert_eq!(SubjectKind::Issue.to_string(), "issue");
        assert_eq!(SubjectKind::Pr.to_string(), "pr");
    }

    #[test]
    fn number_env_var_matches_kind() {
        assert_eq!(make_issue().number_env_var(), "FORZA_ISSUE_NUMBER");
        assert_eq!(make_pr().number_env_var(), "FORZA_PR_NUMBER");
    }

    #[test]
    fn env_vars_include_all_standard_vars() {
        let subject = make_issue();
        let vars = subject.env_vars("run-123", "bugfix", "bug");
        let map: std::collections::HashMap<&str, &str> =
            vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

        assert_eq!(map["FORZA_REPO"], "owner/repo");
        assert_eq!(map["FORZA_SUBJECT_TYPE"], "issue");
        assert_eq!(map["FORZA_SUBJECT_NUMBER"], "42");
        assert_eq!(map["FORZA_SUBJECT_TITLE"], "Fix the bug");
        assert_eq!(map["FORZA_ISSUE_NUMBER"], "42");
        assert_eq!(map["FORZA_BRANCH"], "automation/42-fix-the-bug");
        assert_eq!(map["FORZA_RUN_ID"], "run-123");
        assert_eq!(map["FORZA_ROUTE"], "bugfix");
        assert_eq!(map["FORZA_WORKFLOW"], "bug");
        assert!(!map.contains_key("FORZA_BASE_BRANCH"));
    }

    #[test]
    fn env_vars_pr_includes_base_branch() {
        let subject = make_pr();
        let vars = subject.env_vars("run-456", "auto-merge", "pr-merge");
        let map: std::collections::HashMap<&str, &str> =
            vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

        assert_eq!(map["FORZA_PR_NUMBER"], "99");
        assert_eq!(map["FORZA_BASE_BRANCH"], "main");
        assert_eq!(map["FORZA_SUBJECT_TYPE"], "pr");
    }

    #[test]
    fn has_label() {
        let subject = make_issue();
        assert!(subject.has_label("bug"));
        assert!(subject.has_label("forza:ready"));
        assert!(!subject.has_label("enhancement"));
    }

    #[test]
    fn subject_serialization_roundtrip() {
        let subject = make_pr();
        let json = serde_json::to_string(&subject).unwrap();
        let restored: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.number, 99);
        assert_eq!(restored.kind, SubjectKind::Pr);
        assert_eq!(restored.mergeable.as_deref(), Some("MERGEABLE"));
        assert_eq!(restored.checks_passing, Some(true));
    }
}
