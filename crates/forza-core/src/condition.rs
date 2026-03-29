//! Route conditions for PR state matching.
//!
//! Conditions evaluate whether a PR's current state (CI status, mergeability,
//! review decision) matches a trigger. Used by condition routes to automatically
//! detect PRs that need action.

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::subject::Subject;

/// A condition that triggers a route based on PR state.
///
/// Each variant maps to a specific combination of CI status, mergeability,
/// and review decision. The poll loop evaluates these against all open PRs
/// in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteCondition {
    /// CI checks are failing on the PR.
    CiFailing,
    /// The PR has merge conflicts.
    HasConflicts,
    /// CI is failing or the PR has conflicts (or both).
    CiFailingOrConflicts,
    /// The PR is approved and CI is green.
    ApprovedAndGreen,
    /// CI is green, no conflicts, and no CHANGES_REQUESTED review decision.
    /// Does not require an explicit APPROVED decision — branch protection is the real gate.
    CiGreenNoObjections,
}

impl RouteCondition {
    /// Evaluate whether this condition holds for the given subject.
    ///
    /// Returns `false` if mergeability hasn't been resolved yet (`UNKNOWN` or `None`).
    /// This prevents premature dispatch into workflows that can't act on unresolved state.
    pub fn matches(&self, subject: &Subject) -> bool {
        // Guard: mergeability must be resolved before any condition can match.
        // GitHub returns UNKNOWN or None briefly after PR creation/update.
        match subject.mergeable.as_deref() {
            Some("MERGEABLE") | Some("CONFLICTING") => {}
            Some(other) => {
                debug!(
                    number = subject.number,
                    mergeable = other,
                    "mergeability not yet resolved, skipping cycle"
                );
                return false;
            }
            None => {
                debug!(
                    number = subject.number,
                    mergeable = "None",
                    "mergeability not yet resolved, skipping cycle"
                );
                return false;
            }
        }

        let ci_failing = subject.checks_passing == Some(false);
        let ci_green = subject.checks_passing == Some(true);
        let has_conflicts = subject.mergeable.as_deref() == Some("CONFLICTING");
        let approved = subject.review_decision.as_deref() == Some("APPROVED");
        let changes_requested = subject.review_decision.as_deref() == Some("CHANGES_REQUESTED");

        match self {
            RouteCondition::CiFailing => ci_failing,
            RouteCondition::HasConflicts => has_conflicts,
            RouteCondition::CiFailingOrConflicts => ci_failing || has_conflicts,
            RouteCondition::ApprovedAndGreen => approved && ci_green && !has_conflicts,
            RouteCondition::CiGreenNoObjections => ci_green && !has_conflicts && !changes_requested,
        }
    }
}

impl std::fmt::Display for RouteCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteCondition::CiFailing => f.write_str("ci_failing"),
            RouteCondition::HasConflicts => f.write_str("has_conflicts"),
            RouteCondition::CiFailingOrConflicts => f.write_str("ci_failing_or_conflicts"),
            RouteCondition::ApprovedAndGreen => f.write_str("approved_and_green"),
            RouteCondition::CiGreenNoObjections => f.write_str("ci_green_no_objections"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subject::SubjectKind;

    fn make_pr(
        mergeable: Option<&str>,
        checks_passing: Option<bool>,
        review_decision: Option<&str>,
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
            branch: "fix/thing".into(),
            comments: Vec::new(),
            mergeable: mergeable.map(String::from),
            checks_passing,
            review_decision: review_decision.map(String::from),
            is_draft: Some(false),
            base_branch: Some("main".into()),
        }
    }

    // ── Mergeability guard ──────────────────────────────────────────

    #[test]
    fn unknown_mergeable_blocks_all_conditions() {
        let pr = make_pr(Some("UNKNOWN"), Some(true), None);
        assert!(!RouteCondition::CiFailing.matches(&pr));
        assert!(!RouteCondition::HasConflicts.matches(&pr));
        assert!(!RouteCondition::CiFailingOrConflicts.matches(&pr));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn none_mergeable_blocks_all_conditions() {
        let pr = make_pr(None, Some(true), None);
        assert!(!RouteCondition::CiFailing.matches(&pr));
        assert!(!RouteCondition::HasConflicts.matches(&pr));
        assert!(!RouteCondition::CiFailingOrConflicts.matches(&pr));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    // ── CiFailing ───────────────────────────────────────────────────

    #[test]
    fn ci_failing_matches_when_checks_fail() {
        let pr = make_pr(Some("MERGEABLE"), Some(false), None);
        assert!(RouteCondition::CiFailing.matches(&pr));
    }

    #[test]
    fn ci_failing_does_not_match_when_green() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), None);
        assert!(!RouteCondition::CiFailing.matches(&pr));
    }

    #[test]
    fn ci_failing_does_not_match_when_pending() {
        let pr = make_pr(Some("MERGEABLE"), None, None);
        assert!(!RouteCondition::CiFailing.matches(&pr));
    }

    // ── HasConflicts ────────────────────────────────────────────────

    #[test]
    fn has_conflicts_matches_conflicting() {
        let pr = make_pr(Some("CONFLICTING"), Some(true), None);
        assert!(RouteCondition::HasConflicts.matches(&pr));
    }

    #[test]
    fn has_conflicts_does_not_match_mergeable() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), None);
        assert!(!RouteCondition::HasConflicts.matches(&pr));
    }

    // ── CiFailingOrConflicts ────────────────────────────────────────

    #[test]
    fn ci_failing_or_conflicts_matches_either() {
        let failing = make_pr(Some("MERGEABLE"), Some(false), None);
        assert!(RouteCondition::CiFailingOrConflicts.matches(&failing));

        let conflicting = make_pr(Some("CONFLICTING"), Some(true), None);
        assert!(RouteCondition::CiFailingOrConflicts.matches(&conflicting));

        let both = make_pr(Some("CONFLICTING"), Some(false), None);
        assert!(RouteCondition::CiFailingOrConflicts.matches(&both));
    }

    #[test]
    fn ci_failing_or_conflicts_does_not_match_clean() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), None);
        assert!(!RouteCondition::CiFailingOrConflicts.matches(&pr));
    }

    // ── ApprovedAndGreen ────────────────────────────────────────────

    #[test]
    fn approved_and_green_matches() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), Some("APPROVED"));
        assert!(RouteCondition::ApprovedAndGreen.matches(&pr));
    }

    #[test]
    fn approved_and_green_requires_approval() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), None);
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
    }

    #[test]
    fn approved_and_green_requires_ci_green() {
        let pr = make_pr(Some("MERGEABLE"), Some(false), Some("APPROVED"));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
    }

    #[test]
    fn approved_and_green_blocked_by_conflicts() {
        let pr = make_pr(Some("CONFLICTING"), Some(true), Some("APPROVED"));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
    }

    // ── CiGreenNoObjections ─────────────────────────────────────────

    #[test]
    fn ci_green_no_objections_matches_no_review() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), None);
        assert!(RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn ci_green_no_objections_matches_approved() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), Some("APPROVED"));
        assert!(RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn ci_green_no_objections_blocked_by_changes_requested() {
        let pr = make_pr(Some("MERGEABLE"), Some(true), Some("CHANGES_REQUESTED"));
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn ci_green_no_objections_requires_ci_green() {
        let pr = make_pr(Some("MERGEABLE"), Some(false), None);
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn ci_green_no_objections_pending_ci_does_not_match() {
        let pr = make_pr(Some("MERGEABLE"), None, None);
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn ci_green_no_objections_blocked_by_conflicts() {
        let pr = make_pr(Some("CONFLICTING"), Some(true), None);
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    // ── Display ─────────────────────────────────────────────────────

    #[test]
    fn condition_display() {
        assert_eq!(RouteCondition::CiFailing.to_string(), "ci_failing");
        assert_eq!(
            RouteCondition::CiGreenNoObjections.to_string(),
            "ci_green_no_objections"
        );
    }

    // ── Serialization ───────────────────────────────────────────────

    #[test]
    fn condition_serialization_roundtrip() {
        let cond = RouteCondition::CiFailingOrConflicts;
        let json = serde_json::to_string(&cond).unwrap();
        assert_eq!(json, "\"ci_failing_or_conflicts\"");
        let restored: RouteCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, cond);
    }
}
