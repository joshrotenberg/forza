//! Triage engine — evaluate issue readiness and decide next action.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::github::IssueCandidate;
use crate::policy::RepoPolicy;

static DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:depends\s+on|blocked\s+by)\s+#(\d+)").unwrap()
});

/// Parse dependency issue numbers from an issue body.
///
/// Recognises `depends on #N` and `blocked by #N` (case-insensitive).
pub fn parse_dependencies(body: &str) -> Vec<u64> {
    DEP_RE
        .captures_iter(body)
        .filter_map(|cap| cap[1].parse().ok())
        .collect()
}

/// The outcome of triaging an issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriageDecision {
    /// Issue is ready to plan and execute.
    Ready,
    /// Issue needs clarification — includes questions to ask.
    NeedsClarification(Vec<String>),
    /// Issue is blocked by another issue or external dependency.
    Blocked(String),
    /// Issue is a duplicate.
    Duplicate(u64),
    /// Issue is out of scope for automation.
    OutOfScope(String),
    /// Issue is already in progress (has an active lease or PR).
    AlreadyInProgress,
}

/// Evaluate whether an issue is ready for automation.
pub fn triage(
    issue: &IssueCandidate,
    policy: &RepoPolicy,
    allowed_authors: &[String],
    open_deps: &[u64],
) -> TriageDecision {
    // Check open dependencies first.
    if !open_deps.is_empty() {
        let list = open_deps
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        return TriageDecision::Blocked(format!("waiting on open issues: {list}"));
    }

    // Check author allowlist.
    if !allowed_authors.is_empty() && !allowed_authors.iter().any(|a| a == &issue.author) {
        return TriageDecision::OutOfScope(format!(
            "author '{}' is not in allowed_authors",
            issue.author
        ));
    }

    // Check exclusion labels.
    for label in &issue.labels {
        if policy.exclude_labels.contains(label) {
            return TriageDecision::OutOfScope(format!("excluded by label: {label}"));
        }
    }

    // Check eligibility labels (if configured, at least one must match).
    if !policy.eligible_labels.is_empty()
        && !issue
            .labels
            .iter()
            .any(|l| policy.eligible_labels.contains(l))
    {
        return TriageDecision::OutOfScope("no eligible label".to_string());
    }

    // Check if already assigned.
    if issue.is_assigned {
        return TriageDecision::AlreadyInProgress;
    }

    // Check body quality — very basic heuristic.
    if issue.body.trim().len() < 20 {
        return TriageDecision::NeedsClarification(vec![
            "The issue description is too brief. Can you provide more detail about the expected behavior and acceptance criteria?".to_string(),
        ]);
    }

    TriageDecision::Ready
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_policy() -> crate::policy::RepoPolicy {
        toml::from_str(r#"repo = "owner/repo""#).unwrap()
    }

    fn make_issue(author: &str) -> IssueCandidate {
        IssueCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "some issue".into(),
            body: "This is a detailed issue description with enough text.".into(),
            labels: vec![],
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            author: author.into(),
            comments: vec![],
        }
    }

    #[test]
    fn allowed_authors_empty_does_not_filter() {
        let policy = make_policy();
        let issue = make_issue("anyone");
        let result = triage(&issue, &policy, &[], &[]);
        assert!(matches!(result, TriageDecision::Ready));
    }

    #[test]
    fn allowed_authors_matching_author_passes() {
        let policy = make_policy();
        let issue = make_issue("alice");
        let allowed = vec!["alice".to_string(), "bob".to_string()];
        let result = triage(&issue, &policy, &allowed, &[]);
        assert!(matches!(result, TriageDecision::Ready));
    }

    #[test]
    fn allowed_authors_non_matching_author_rejected() {
        let policy = make_policy();
        let issue = make_issue("mallory");
        let allowed = vec!["alice".to_string(), "bob".to_string()];
        let result = triage(&issue, &policy, &allowed, &[]);
        assert!(matches!(result, TriageDecision::OutOfScope(_)));
    }

    #[test]
    fn open_deps_returns_blocked() {
        let policy = make_policy();
        let issue = make_issue("alice");
        let result = triage(&issue, &policy, &[], &[42]);
        assert!(matches!(result, TriageDecision::Blocked(_)));
    }

    #[test]
    fn no_open_deps_proceeds_normally() {
        let policy = make_policy();
        let issue = make_issue("alice");
        let result = triage(&issue, &policy, &[], &[]);
        assert!(matches!(result, TriageDecision::Ready));
    }

    #[test]
    fn parse_dependencies_depends_on() {
        let body = "This depends on #12 and also depends on #34";
        let deps = parse_dependencies(body);
        assert_eq!(deps, vec![12, 34]);
    }

    #[test]
    fn parse_dependencies_blocked_by() {
        let body = "Blocked by #99";
        let deps = parse_dependencies(body);
        assert_eq!(deps, vec![99]);
    }

    #[test]
    fn parse_dependencies_mixed_case() {
        let body = "DEPENDS ON #5\nBLOCKED BY #6";
        let deps = parse_dependencies(body);
        assert_eq!(deps, vec![5, 6]);
    }

    #[test]
    fn parse_dependencies_none() {
        let body = "No dependencies here.";
        let deps = parse_dependencies(body);
        assert!(deps.is_empty());
    }
}
