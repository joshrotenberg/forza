//! Run records — the persistent trace of every workflow execution.
//!
//! Every field is public and serializable. REST, MCP, metrics, and the CLI
//! can access any piece of data from a run without going through accessors.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::stage::StageKind;
use crate::subject::SubjectKind;

/// The overall status of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// All stages completed successfully (or were optional).
    Succeeded,
    /// A non-optional stage failed.
    Failed,
    /// The run is still in progress.
    Running,
    /// The run was cancelled before execution (e.g. gate label removed).
    Skipped,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Succeeded => f.write_str("succeeded"),
            RunStatus::Failed => f.write_str("failed"),
            RunStatus::Running => f.write_str("running"),
            RunStatus::Skipped => f.write_str("skipped"),
        }
    }
}

/// The business outcome of a completed run.
///
/// Describes what actually happened, not just pass/fail. Used by notifications,
/// status display, and retry budget tracking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A new PR was created from an issue.
    PrCreated { number: u64 },
    /// An existing PR was updated (rebased, CI fixed, etc.).
    PrUpdated { number: u64 },
    /// A PR was merged.
    PrMerged { number: u64 },
    /// A comment was posted (research, triage).
    CommentPosted,
    /// No action was needed this cycle.
    NothingToDo,
    /// The run failed at a specific stage.
    Failed { stage: String, error: String },
    /// Retry budget exhausted — `forza:needs-human` applied.
    Exhausted { retries: usize },
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::PrCreated { number } => write!(f, "pr_created (#{number})"),
            Outcome::PrUpdated { number } => write!(f, "pr_updated (#{number})"),
            Outcome::PrMerged { number } => write!(f, "pr_merged (#{number})"),
            Outcome::CommentPosted => f.write_str("comment_posted"),
            Outcome::NothingToDo => f.write_str("nothing_to_do"),
            Outcome::Failed { stage, .. } => write!(f, "failed (stage: {stage})"),
            Outcome::Exhausted { retries } => write!(f, "exhausted ({retries} retries)"),
        }
    }
}

/// The execution status of a single stage within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    /// Stage completed successfully.
    Succeeded,
    /// Stage failed.
    Failed,
    /// Stage was skipped (condition not met, optional).
    Skipped,
}

impl std::fmt::Display for StageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageStatus::Succeeded => f.write_str("succeeded"),
            StageStatus::Failed => f.write_str("failed"),
            StageStatus::Skipped => f.write_str("skipped"),
        }
    }
}

/// The result of executing a single stage.
///
/// Captures everything about what happened during the stage for later
/// analysis, debugging, and metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// The name of the stage that produced this result.
    pub stage: String,
    /// Whether the stage succeeded.
    pub success: bool,
    /// Wall-clock duration in seconds.
    pub duration_secs: f64,
    /// Estimated cost in USD (agent stages only).
    pub cost_usd: Option<f64>,
    /// Captured output (stdout/stderr for shell, summary for agent).
    pub output: String,
    /// Files modified by this stage.
    pub files_modified: Option<Vec<String>>,
}

/// Record of a single stage's execution within a run.
///
/// Combines the stage identity, its execution status, and the detailed result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRecord {
    /// Which stage kind was executed.
    pub kind: StageKind,
    /// Pass/fail/skip.
    pub status: StageStatus,
    /// Number of attempts (for retried stages).
    pub attempts: u32,
    /// The detailed execution result, if the stage ran.
    pub result: Option<StageResult>,
}

impl StageRecord {
    /// Human-readable name for this stage.
    pub fn kind_name(&self) -> &str {
        self.kind.name()
    }
}

/// A complete record of a workflow execution.
///
/// Every field is public and serializable. This is the primary data structure
/// surfaced by `forza status`, the REST API, MCP tools, and notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// Unique identifier for this run.
    pub run_id: String,
    /// Repository this run operated on.
    pub repo: String,
    /// Issue or PR number.
    pub subject_number: u64,
    /// Whether the subject is an issue or PR.
    pub subject_kind: SubjectKind,
    /// The route that triggered this run.
    pub route: String,
    /// The workflow that was executed.
    pub workflow: String,
    /// The branch used for this run.
    pub branch: String,
    /// PR number, if a PR was created or updated.
    pub pr_number: Option<u64>,
    /// Per-stage execution records.
    pub stages: Vec<StageRecord>,
    /// Overall status.
    pub status: RunStatus,
    /// Business outcome.
    pub outcome: Option<Outcome>,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Total estimated cost across all agent stages.
    pub total_cost_usd: Option<f64>,
}

impl Run {
    /// Create a new run record in the Running state.
    pub fn new(
        run_id: impl Into<String>,
        repo: impl Into<String>,
        subject_number: u64,
        subject_kind: SubjectKind,
        route: impl Into<String>,
        workflow: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            repo: repo.into(),
            subject_number,
            subject_kind,
            route: route.into(),
            workflow: workflow.into(),
            branch: branch.into(),
            pr_number: None,
            stages: Vec::new(),
            status: RunStatus::Running,
            outcome: None,
            started_at: Utc::now(),
            completed_at: None,
            total_cost_usd: None,
        }
    }

    /// Record a stage result.
    pub fn record_stage(&mut self, kind: StageKind, status: StageStatus, result: StageResult) {
        self.stages.push(StageRecord {
            kind,
            status,
            attempts: 1,
            result: Some(result),
        });
    }

    /// Record a skipped stage (condition not met).
    pub fn record_skipped(&mut self, kind: StageKind) {
        self.stages.push(StageRecord {
            kind,
            status: StageStatus::Skipped,
            attempts: 0,
            result: None,
        });
    }

    /// Mark the run as finished, computing final status and cost.
    pub fn finish(&mut self, status: RunStatus) {
        self.status = status;
        self.completed_at = Some(Utc::now());
        self.total_cost_usd = self.compute_total_cost();
    }

    /// Whether all stages that ran succeeded.
    pub fn all_stages_succeeded(&self) -> bool {
        self.stages
            .iter()
            .all(|s| s.status == StageStatus::Succeeded || s.status == StageStatus::Skipped)
    }

    /// Duration of the run, if completed.
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.started_at)
    }

    /// Duration as a human-readable string.
    pub fn duration_display(&self) -> String {
        match self.duration() {
            Some(d) => {
                let secs = d.num_seconds();
                if secs < 60 {
                    format!("{secs}s")
                } else {
                    format!("{}m {}s", secs / 60, secs % 60)
                }
            }
            None => "running".into(),
        }
    }

    /// Compute total cost from all stage results.
    fn compute_total_cost(&self) -> Option<f64> {
        let costs: Vec<f64> = self
            .stages
            .iter()
            .filter_map(|s| s.result.as_ref()?.cost_usd)
            .collect();
        if costs.is_empty() {
            None
        } else {
            Some(costs.iter().sum())
        }
    }

    /// The stage that failed, if any.
    pub fn failed_stage(&self) -> Option<&StageRecord> {
        self.stages.iter().find(|s| s.status == StageStatus::Failed)
    }
}

/// Generate a unique run ID.
pub fn generate_run_id() -> String {
    let now = Utc::now();
    let timestamp = now.format("%Y%m%d-%H%M%S");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let suffix = (nanos ^ (nanos >> 24)) & 0xFFFF_FFFF;
    format!("run-{timestamp}-{suffix:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_run() -> Run {
        Run::new(
            "run-20260322-120000-abcd1234",
            "owner/repo",
            42,
            SubjectKind::Issue,
            "bugfix",
            "bug",
            "automation/42-fix-bug",
        )
    }

    fn make_result(success: bool, duration: f64, cost: Option<f64>) -> StageResult {
        StageResult {
            stage: "test".into(),
            success,
            duration_secs: duration,
            cost_usd: cost,
            output: String::new(),
            files_modified: None,
        }
    }

    #[test]
    fn new_run_is_running() {
        let run = make_run();
        assert_eq!(run.status, RunStatus::Running);
        assert!(run.completed_at.is_none());
        assert!(run.stages.is_empty());
    }

    #[test]
    fn record_stage_adds_to_stages() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 10.0, Some(0.50)),
        );
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.stages[0].kind, StageKind::Plan);
        assert_eq!(run.stages[0].status, StageStatus::Succeeded);
    }

    #[test]
    fn record_skipped_has_no_result() {
        let mut run = make_run();
        run.record_skipped(StageKind::Test);
        assert_eq!(run.stages[0].status, StageStatus::Skipped);
        assert!(run.stages[0].result.is_none());
        assert_eq!(run.stages[0].attempts, 0);
    }

    #[test]
    fn finish_sets_completed_at() {
        let mut run = make_run();
        run.finish(RunStatus::Succeeded);
        assert!(run.completed_at.is_some());
        assert_eq!(run.status, RunStatus::Succeeded);
    }

    #[test]
    fn all_stages_succeeded_with_skips() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 5.0, None),
        );
        run.record_skipped(StageKind::Test);
        run.record_stage(
            StageKind::Implement,
            StageStatus::Succeeded,
            make_result(true, 20.0, None),
        );
        assert!(run.all_stages_succeeded());
    }

    #[test]
    fn all_stages_succeeded_false_on_failure() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 5.0, None),
        );
        run.record_stage(
            StageKind::Implement,
            StageStatus::Failed,
            make_result(false, 20.0, None),
        );
        assert!(!run.all_stages_succeeded());
    }

    #[test]
    fn total_cost_sums_agent_stages() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 5.0, Some(0.50)),
        );
        run.record_stage(
            StageKind::Implement,
            StageStatus::Succeeded,
            make_result(true, 20.0, Some(1.25)),
        );
        run.record_stage(
            StageKind::Test,
            StageStatus::Succeeded,
            make_result(true, 10.0, None), // agentless, no cost
        );
        run.finish(RunStatus::Succeeded);
        assert_eq!(run.total_cost_usd, Some(1.75));
    }

    #[test]
    fn total_cost_none_when_no_agent_stages() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Merge,
            StageStatus::Succeeded,
            make_result(true, 2.0, None),
        );
        run.finish(RunStatus::Succeeded);
        assert_eq!(run.total_cost_usd, None);
    }

    #[test]
    fn failed_stage_returns_first_failure() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 5.0, None),
        );
        run.record_stage(
            StageKind::Implement,
            StageStatus::Failed,
            make_result(false, 20.0, None),
        );
        let failed = run.failed_stage().unwrap();
        assert_eq!(failed.kind, StageKind::Implement);
    }

    #[test]
    fn duration_display_formats_correctly() {
        let mut run = make_run();
        // Simulate a 90-second run
        run.completed_at = Some(run.started_at + chrono::Duration::seconds(90));
        assert_eq!(run.duration_display(), "1m 30s");
    }

    #[test]
    fn outcome_display() {
        assert_eq!(
            Outcome::PrCreated { number: 42 }.to_string(),
            "pr_created (#42)"
        );
        assert_eq!(Outcome::NothingToDo.to_string(), "nothing_to_do");
        assert_eq!(
            Outcome::Failed {
                stage: "implement".into(),
                error: "crash".into()
            }
            .to_string(),
            "failed (stage: implement)"
        );
        assert_eq!(
            Outcome::Exhausted { retries: 3 }.to_string(),
            "exhausted (3 retries)"
        );
    }

    #[test]
    fn run_serialization_roundtrip() {
        let mut run = make_run();
        run.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_result(true, 5.0, Some(0.50)),
        );
        run.finish(RunStatus::Succeeded);
        run.outcome = Some(Outcome::PrCreated { number: 99 });

        let json = serde_json::to_string_pretty(&run).unwrap();
        let restored: Run = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.run_id, run.run_id);
        assert_eq!(restored.subject_number, 42);
        assert_eq!(restored.subject_kind, SubjectKind::Issue);
        assert_eq!(restored.stages.len(), 1);
        assert_eq!(restored.status, RunStatus::Succeeded);
        assert_eq!(restored.outcome, Some(Outcome::PrCreated { number: 99 }));
    }

    #[test]
    fn generate_run_id_is_unique() {
        let a = generate_run_id();
        let b = generate_run_id();
        assert_ne!(a, b);
        assert!(a.starts_with("run-"));
    }

    #[test]
    fn run_status_display() {
        assert_eq!(RunStatus::Succeeded.to_string(), "succeeded");
        assert_eq!(RunStatus::Failed.to_string(), "failed");
        assert_eq!(RunStatus::Running.to_string(), "running");
        assert_eq!(RunStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn stage_status_display() {
        assert_eq!(StageStatus::Succeeded.to_string(), "succeeded");
        assert_eq!(StageStatus::Failed.to_string(), "failed");
        assert_eq!(StageStatus::Skipped.to_string(), "skipped");
    }
}
