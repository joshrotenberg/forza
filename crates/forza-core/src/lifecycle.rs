//! Lifecycle label management.
//!
//! Manages the `forza:in-progress`, `forza:complete`, `forza:failed`, and
//! `forza:needs-human` labels that track a subject's processing state.
//! All label operations are best-effort (non-fatal on failure).

use tracing::{info, warn};

use crate::run::{Run, RunStatus};
use crate::subject::Subject;
use crate::traits::GitHubClient;

/// Default label names. These can be overridden in config.
pub const DEFAULT_IN_PROGRESS: &str = "forza:in-progress";
pub const DEFAULT_COMPLETE: &str = "forza:complete";
pub const DEFAULT_FAILED: &str = "forza:failed";
pub const DEFAULT_NEEDS_HUMAN: &str = "forza:needs-human";

/// Label configuration for lifecycle management.
#[derive(Debug, Clone)]
pub struct LifecycleLabels {
    /// Applied when processing starts.
    pub in_progress: String,
    /// Applied on successful completion.
    pub complete: String,
    /// Applied on failure.
    pub failed: String,
    /// Applied when retry budget is exhausted.
    pub needs_human: String,
    /// Gate label to remove when processing starts.
    pub gate: Option<String>,
}

impl Default for LifecycleLabels {
    fn default() -> Self {
        Self {
            in_progress: DEFAULT_IN_PROGRESS.into(),
            complete: DEFAULT_COMPLETE.into(),
            failed: DEFAULT_FAILED.into(),
            needs_human: DEFAULT_NEEDS_HUMAN.into(),
            gate: None,
        }
    }
}

/// Acquire the processing lease: add in-progress, remove gate label.
pub async fn acquire(subject: &Subject, labels: &LifecycleLabels, gh: &dyn GitHubClient) {
    let repo = &subject.repo;
    let number = subject.number;

    if let Err(e) = gh.add_label(repo, number, &labels.in_progress).await {
        warn!(number, error = %e, "failed to add in-progress label (non-fatal)");
    } else {
        info!(number, label = %labels.in_progress, "acquired processing lease");
    }

    if let Some(gate) = &labels.gate {
        let _ = gh.remove_label(repo, number, gate).await;
    }
}

/// Release the processing lease: remove in-progress, add outcome label.
pub async fn release(
    subject: &Subject,
    run: &Run,
    labels: &LifecycleLabels,
    gh: &dyn GitHubClient,
) {
    let repo = &subject.repo;
    let number = subject.number;

    // Remove in-progress.
    let _ = gh.remove_label(repo, number, &labels.in_progress).await;

    // Apply outcome label.
    let outcome_label = match run.status {
        RunStatus::Succeeded => &labels.complete,
        RunStatus::Failed => &labels.failed,
        RunStatus::Running => return, // still running, don't touch labels
        RunStatus::Skipped => return, // cancelled before execution, don't touch labels
    };

    if let Err(e) = gh.add_label(repo, number, outcome_label).await {
        warn!(number, label = %outcome_label, error = %e, "failed to add outcome label (non-fatal)");
    }
}

/// Apply the needs-human label when retry budget is exhausted.
pub async fn escalate(
    subject: &Subject,
    labels: &LifecycleLabels,
    route_name: &str,
    retries: usize,
    max_retries: usize,
    gh: &dyn GitHubClient,
) {
    let repo = &subject.repo;
    let number = subject.number;

    if let Err(e) = gh.add_label(repo, number, &labels.needs_human).await {
        warn!(number, error = %e, "failed to add needs-human label (non-fatal)");
    }

    let comment = format!(
        "Retry budget exhausted for route `{route_name}` \
         ({retries}/{max_retries} attempts). Applying `{}` for manual review.",
        labels.needs_human
    );
    let _ = gh.post_comment(repo, number, &comment).await;
}

/// Check if a subject has any lifecycle label that should prevent processing.
pub fn is_blocked(subject: &Subject, labels: &LifecycleLabels) -> bool {
    subject.has_label(&labels.in_progress)
        || subject.has_label(&labels.complete)
        || subject.has_label(&labels.needs_human)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_subject(subject_labels: &[&str]) -> Subject {
        Subject {
            kind: crate::subject::SubjectKind::Issue,
            number: 42,
            repo: "owner/repo".into(),
            title: "Test".into(),
            body: String::new(),
            labels: subject_labels.iter().map(|s| s.to_string()).collect(),
            html_url: String::new(),
            author: "user".into(),
            branch: "automation/42-test".into(),
            comments: Vec::new(),
            mergeable: None,
            checks_passing: None,
            review_decision: None,
            is_draft: None,
            base_branch: None,
        }
    }

    #[test]
    fn default_labels() {
        let labels = LifecycleLabels::default();
        assert_eq!(labels.in_progress, "forza:in-progress");
        assert_eq!(labels.complete, "forza:complete");
        assert_eq!(labels.failed, "forza:failed");
        assert_eq!(labels.needs_human, "forza:needs-human");
        assert!(labels.gate.is_none());
    }

    #[test]
    fn is_blocked_by_in_progress() {
        let labels = LifecycleLabels::default();
        let subject = make_subject(&["forza:in-progress"]);
        assert!(is_blocked(&subject, &labels));
    }

    #[test]
    fn is_blocked_by_complete() {
        let labels = LifecycleLabels::default();
        let subject = make_subject(&["forza:complete"]);
        assert!(is_blocked(&subject, &labels));
    }

    #[test]
    fn is_blocked_by_needs_human() {
        let labels = LifecycleLabels::default();
        let subject = make_subject(&["forza:needs-human"]);
        assert!(is_blocked(&subject, &labels));
    }

    #[test]
    fn not_blocked_without_lifecycle_labels() {
        let labels = LifecycleLabels::default();
        let subject = make_subject(&["bug", "forza:ready"]);
        assert!(!is_blocked(&subject, &labels));
    }

    #[test]
    fn not_blocked_by_failed() {
        // failed subjects CAN be retried
        let labels = LifecycleLabels::default();
        let subject = make_subject(&["forza:failed"]);
        assert!(!is_blocked(&subject, &labels));
    }
}
