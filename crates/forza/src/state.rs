//! Run state — persisted records of execution attempts.

use serde::{Deserialize, Serialize};

use crate::executor::StageResult;
use crate::workflow::StageKind;

/// Whether the run subject is an issue or a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    #[default]
    Issue,
    Pr,
}

/// State of an issue in the automation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    New,
    Triaging,
    NeedsClarification,
    Ready,
    Planning,
    InProgress,
    WaitingOnReview,
    WaitingOnHuman,
    Blocked,
    Completed,
    ClosedUnresolved,
}

/// State of a single run attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Leased,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Abandoned,
}

/// State of a single stage within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Waiting,
}

/// The outcome of a route execution — what the run produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteOutcome {
    /// A new PR was created from an issue workflow.
    PrCreated { number: u64 },
    /// An existing PR was updated (rebased, CI fixed, etc.).
    PrUpdated { number: u64 },
    /// A PR was merged.
    PrMerged { number: u64 },
    /// A comment was posted (e.g., research workflow).
    CommentPosted,
    /// No action was needed (e.g., reactive mode found nothing to do).
    NothingToDo,
    /// The run failed at a specific stage.
    Failed { stage: String, error: String },
    /// Retry budget was exhausted — needs human intervention.
    Exhausted { retries: usize },
}

/// A persisted run record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    /// Stable run identifier.
    pub run_id: String,
    /// Repository.
    pub repo: String,
    /// Issue number.
    pub issue_number: u64,
    /// Overall run status.
    pub status: RunStatus,
    /// Workflow template used.
    pub workflow: String,
    /// Branch name.
    pub branch: String,
    /// PR number if created.
    pub pr_number: Option<u64>,
    /// Per-stage results.
    pub stages: Vec<StageRecord>,
    /// When the run started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the run completed.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Total cost across all stages.
    pub total_cost_usd: Option<f64>,
    /// Whether this run is for an issue or a PR.
    #[serde(default)]
    pub subject_kind: SubjectKind,
    /// The outcome of the run — what it produced.
    #[serde(default)]
    pub outcome: Option<RouteOutcome>,
}

/// Per-stage record within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRecord {
    /// Stage kind.
    pub kind: StageKind,
    /// Stage status.
    pub status: StageStatus,
    /// Attempt count.
    pub attempts: u32,
    /// Result from the last attempt (if any).
    pub result: Option<StageResult>,
}

impl StageRecord {
    /// Human-readable name.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
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

impl RunRecord {
    /// Human-readable status text.
    pub fn status_text(&self) -> &'static str {
        match self.status {
            RunStatus::Queued => "queued",
            RunStatus::Leased => "leased",
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::Canceled => "canceled",
            RunStatus::Abandoned => "abandoned",
        }
    }

    /// Create a new run record for an issue.
    pub fn new(run_id: &str, repo: &str, issue_number: u64, workflow: &str, branch: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            repo: repo.to_string(),
            issue_number,
            status: RunStatus::Running,
            workflow: workflow.to_string(),
            branch: branch.to_string(),
            pr_number: None,
            stages: Vec::new(),
            started_at: chrono::Utc::now(),
            completed_at: None,
            total_cost_usd: None,
            subject_kind: SubjectKind::Issue,
            outcome: None,
        }
    }

    /// Create a new run record for a PR.
    pub fn new_for_pr(
        run_id: &str,
        repo: &str,
        pr_number: u64,
        workflow: &str,
        branch: &str,
    ) -> Self {
        Self {
            run_id: run_id.to_string(),
            repo: repo.to_string(),
            issue_number: pr_number,
            status: RunStatus::Running,
            workflow: workflow.to_string(),
            branch: branch.to_string(),
            pr_number: Some(pr_number),
            stages: Vec::new(),
            started_at: chrono::Utc::now(),
            completed_at: None,
            total_cost_usd: None,
            subject_kind: SubjectKind::Pr,
            outcome: None,
        }
    }

    /// Record a stage result.
    pub fn record_stage(&mut self, kind: StageKind, status: StageStatus, result: StageResult) {
        // Update or append.
        if let Some(existing) = self.stages.iter_mut().find(|s| s.kind == kind) {
            existing.status = status;
            existing.attempts += 1;
            existing.result = Some(result);
        } else {
            self.stages.push(StageRecord {
                kind,
                status,
                attempts: 1,
                result: Some(result),
            });
        }
    }

    /// Record a stage as skipped (no result).
    pub fn record_stage_skipped(&mut self, kind: StageKind) {
        if let Some(existing) = self.stages.iter_mut().find(|s| s.kind == kind) {
            existing.status = StageStatus::Skipped;
        } else {
            self.stages.push(StageRecord {
                kind,
                status: StageStatus::Skipped,
                attempts: 0,
                result: None,
            });
        }
    }

    /// Returns the sum of costs recorded so far across completed stages.
    pub fn accumulated_cost_usd(&self) -> Option<f64> {
        let costs: Vec<f64> = self
            .stages
            .iter()
            .filter_map(|s| s.result.as_ref())
            .filter_map(|r| r.cost_usd)
            .collect();
        if costs.is_empty() {
            None
        } else {
            Some(costs.iter().sum())
        }
    }

    /// Mark the run as finished.
    pub fn finish(&mut self, status: RunStatus) {
        self.status = status;
        self.completed_at = Some(chrono::Utc::now());
        self.total_cost_usd = {
            let costs: Vec<f64> = self
                .stages
                .iter()
                .filter_map(|s| s.result.as_ref())
                .filter_map(|r| r.cost_usd)
                .collect();
            if costs.is_empty() {
                None
            } else {
                Some(costs.iter().sum())
            }
        };
    }
}

/// Save a run record to disk.
pub fn save_run(record: &RunRecord, state_dir: &std::path::Path) -> crate::error::Result<()> {
    std::fs::create_dir_all(state_dir)?;

    // Write run record atomically.
    let final_path = state_dir.join(format!("{}.json", record.run_id));
    let tmp_path = state_dir.join(format!("{}.json.tmp", record.run_id));
    let json = serde_json::to_string_pretty(record)?;
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &final_path)?;

    // Update latest pointer atomically.
    let latest_tmp = state_dir.join("latest.tmp");
    std::fs::write(&latest_tmp, &record.run_id)?;
    std::fs::rename(&latest_tmp, state_dir.join("latest"))?;

    Ok(())
}

/// Load the most recent run.
pub fn load_latest(state_dir: &std::path::Path) -> Option<RunRecord> {
    let latest = state_dir.join("latest");
    let run_id = std::fs::read_to_string(latest).ok()?;
    load_run(run_id.trim(), state_dir)
}

/// Load a specific run by ID.
pub fn load_run(run_id: &str, state_dir: &std::path::Path) -> Option<RunRecord> {
    let path = state_dir.join(format!("{run_id}.json"));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Load all runs from the state directory, sorted by `started_at` descending.
pub fn load_all_runs(state_dir: &std::path::Path) -> Vec<RunRecord> {
    let entries = match std::fs::read_dir(state_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut records: Vec<RunRecord> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            serde_json::from_str(&content).ok()
        })
        .collect();
    records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    records
}

/// Find the most recent run for a given issue number.
pub fn find_latest_run_for_issue(
    issue_number: u64,
    state_dir: &std::path::Path,
) -> Option<RunRecord> {
    load_all_runs(state_dir)
        .into_iter()
        .find(|r| r.issue_number == issue_number)
}

/// Find the most recent completed run for a given PR number and workflow.
///
/// Returns the first (most recent, since `load_all_runs` sorts descending by `started_at`)
/// record that has a `completed_at` timestamp, matching both `issue_number` and `workflow`.
pub fn find_last_completed_run_for_subject(
    pr_number: u64,
    workflow: &str,
    state_dir: &std::path::Path,
) -> Option<RunRecord> {
    load_all_runs(state_dir)
        .into_iter()
        .find(|r| r.issue_number == pr_number && r.workflow == workflow && r.completed_at.is_some())
}

/// Count completed runs for a given issue/PR number with a specific workflow.
pub fn count_runs_for_subject(
    issue_number: u64,
    workflow: &str,
    state_dir: &std::path::Path,
) -> usize {
    load_all_runs(state_dir)
        .iter()
        .filter(|r| r.issue_number == issue_number && r.workflow == workflow)
        .count()
}

/// Count only failed runs (outcome is `Failed` or `Exhausted`) for a given issue/PR number
/// and workflow. Successful outcomes (`PrCreated`, `PrUpdated`, `PrMerged`, `CommentPosted`,
/// `NothingToDo`) do not consume the retry budget.
pub fn count_failed_runs_for_subject(
    issue_number: u64,
    workflow: &str,
    state_dir: &std::path::Path,
) -> usize {
    load_all_runs(state_dir)
        .iter()
        .filter(|r| {
            r.issue_number == issue_number
                && r.workflow == workflow
                && matches!(
                    r.outcome,
                    Some(RouteOutcome::Failed { .. }) | Some(RouteOutcome::Exhausted { .. })
                )
        })
        .count()
}

/// Per-workflow aggregate stats.
#[derive(Debug, Clone)]
pub struct WorkflowSummary {
    /// Workflow name.
    pub workflow: String,
    /// Total number of runs.
    pub total_runs: usize,
    /// Number of succeeded runs.
    pub succeeded: usize,
    /// Number of failed runs.
    pub failed: usize,
    /// Minimum cost across all runs (if any have cost data).
    pub min_cost: Option<f64>,
    /// Maximum cost across all runs (if any have cost data).
    pub max_cost: Option<f64>,
    /// Average cost across all runs (if any have cost data).
    pub avg_cost: Option<f64>,
}

/// Group all runs by workflow and compute per-workflow aggregate stats.
pub fn summarize_by_workflow(state_dir: &std::path::Path) -> Vec<WorkflowSummary> {
    let records = load_all_runs(state_dir);
    let mut map: std::collections::HashMap<String, Vec<RunRecord>> =
        std::collections::HashMap::new();
    for record in records {
        map.entry(record.workflow.clone()).or_default().push(record);
    }
    let mut summaries: Vec<WorkflowSummary> = map
        .into_iter()
        .map(|(workflow, runs)| {
            let total_runs = runs.len();
            let succeeded = runs
                .iter()
                .filter(|r| r.status == RunStatus::Succeeded)
                .count();
            let failed = runs
                .iter()
                .filter(|r| r.status == RunStatus::Failed)
                .count();
            let costs: Vec<f64> = runs.iter().filter_map(|r| r.total_cost_usd).collect();
            let (min_cost, max_cost, avg_cost) = if costs.is_empty() {
                (None, None, None)
            } else {
                let min = costs.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = costs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let avg = costs.iter().sum::<f64>() / costs.len() as f64;
                (Some(min), Some(max), Some(avg))
            };
            WorkflowSummary {
                workflow,
                total_runs,
                succeeded,
                failed,
                min_cost,
                max_cost,
                avg_cost,
            }
        })
        .collect();
    summaries.sort_by(|a, b| a.workflow.cmp(&b.workflow));
    summaries
}

/// Cost estimate derived from historical runs of the same workflow type.
#[derive(Debug, Clone)]
pub struct CostEstimate {
    /// Minimum cost seen.
    pub min: f64,
    /// Maximum cost seen.
    pub max: f64,
    /// Average cost.
    pub avg: f64,
    /// Number of historical runs used.
    pub count: usize,
    /// Workflow name.
    pub workflow: String,
}

/// Compute a cost estimate from previous completed runs of the same workflow type.
///
/// Returns `None` if no historical data is available.
pub fn estimate_cost(workflow: &str, state_dir: &std::path::Path) -> Option<CostEstimate> {
    let entries = std::fs::read_dir(state_dir).ok()?;
    let costs: Vec<f64> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            let record: RunRecord = serde_json::from_str(&content).ok()?;
            if record.workflow == workflow && record.status == RunStatus::Succeeded {
                record.total_cost_usd
            } else {
                None
            }
        })
        .collect();

    if costs.is_empty() {
        return None;
    }

    let min = costs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = costs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg = costs.iter().sum::<f64>() / costs.len() as f64;

    Some(CostEstimate {
        min,
        max,
        avg,
        count: costs.len(),
        workflow: workflow.to_string(),
    })
}

/// Sum `total_cost_usd` for all runs started within the last 60 minutes.
pub fn hourly_cost(state_dir: &std::path::Path) -> f64 {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
    load_all_runs(state_dir)
        .iter()
        .filter(|r| r.started_at >= cutoff)
        .filter_map(|r| r.total_cost_usd)
        .sum()
}

/// List all run state files (*.json and the `latest` pointer) in `state_dir`.
pub fn list_run_files(state_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(state_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }
    let latest = state_dir.join("latest");
    if latest.exists() {
        files.push(latest);
    }
    files
}

pub fn generate_run_id() -> String {
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d-%H%M%S");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let suffix = (nanos ^ (nanos >> 24)) & 0xFFFF_FFFF;
    format!("run-{timestamp}-{suffix:08x}")
}

/// Execution status of a single issue within a plan execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanIssueStatus {
    Succeeded,
    Failed,
    Skipped,
}

/// Per-issue record within a plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanIssueEntry {
    /// Issue number.
    pub issue_number: u64,
    /// Final status of this issue.
    pub status: PlanIssueStatus,
    /// PR number if created or merged.
    pub pr_number: Option<u64>,
    /// Whether the PR was merged.
    pub pr_merged: bool,
    /// Name of the stage that failed, if any.
    pub failed_stage: Option<String>,
}

/// A persisted record of a plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExecRecord {
    /// The plan issue number.
    pub plan_number: u64,
    /// Repository.
    pub repo: String,
    /// When execution started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Per-issue results.
    pub issues: Vec<PlanIssueEntry>,
}

/// Save a plan execution record to disk.
pub fn save_plan_exec(
    record: &PlanExecRecord,
    state_dir: &std::path::Path,
) -> crate::error::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let final_path = state_dir.join(format!("plan_{}.json", record.plan_number));
    let tmp_path = state_dir.join(format!("plan_{}.json.tmp", record.plan_number));
    let json = serde_json::to_string_pretty(record)?;
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

/// Load all plan execution records from the state directory, sorted by `started_at` descending.
pub fn load_all_plan_execs(state_dir: &std::path::Path) -> Vec<PlanExecRecord> {
    let entries = match std::fs::read_dir(state_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut records: Vec<PlanExecRecord> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("plan_") && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            serde_json::from_str(&content).ok()
        })
        .collect();
    records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stage_result(cost: Option<f64>) -> crate::executor::StageResult {
        crate::executor::StageResult {
            stage: "implement".into(),
            success: true,
            duration_secs: 1.0,
            cost_usd: cost,
            output: String::new(),
            files_modified: None,
        }
    }

    fn make_record(workflow: &str, status: RunStatus, cost: Option<f64>) -> RunRecord {
        RunRecord {
            run_id: generate_run_id(),
            repo: "owner/repo".into(),
            issue_number: 1,
            status,
            workflow: workflow.to_string(),
            branch: "fix/test".into(),
            pr_number: None,
            stages: Vec::new(),
            started_at: chrono::Utc::now(),
            completed_at: None,
            total_cost_usd: cost,
            subject_kind: SubjectKind::Issue,
            outcome: None,
        }
    }

    #[test]
    fn accumulated_cost_usd_no_stages() {
        let record = make_record("bug", RunStatus::Running, None);
        assert!(record.accumulated_cost_usd().is_none());
    }

    #[test]
    fn accumulated_cost_usd_stages_without_cost() {
        let mut record = make_record("bug", RunStatus::Running, None);
        record.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_stage_result(None),
        );
        assert!(record.accumulated_cost_usd().is_none());
    }

    #[test]
    fn accumulated_cost_usd_single_stage() {
        let mut record = make_record("bug", RunStatus::Running, None);
        record.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_stage_result(Some(0.50)),
        );
        let total = record.accumulated_cost_usd().unwrap();
        assert!((total - 0.50).abs() < 1e-9);
    }

    #[test]
    fn accumulated_cost_usd_multiple_stages() {
        let mut record = make_record("bug", RunStatus::Running, None);
        record.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_stage_result(Some(0.30)),
        );
        record.record_stage(
            StageKind::Implement,
            StageStatus::Succeeded,
            make_stage_result(Some(0.70)),
        );
        let total = record.accumulated_cost_usd().unwrap();
        assert!((total - 1.00).abs() < 1e-9);
    }

    #[test]
    fn accumulated_cost_usd_mixed_cost_presence() {
        let mut record = make_record("bug", RunStatus::Running, None);
        record.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            make_stage_result(Some(0.40)),
        );
        record.record_stage(
            StageKind::Implement,
            StageStatus::Succeeded,
            make_stage_result(None),
        );
        record.record_stage(
            StageKind::Review,
            StageStatus::Succeeded,
            make_stage_result(Some(0.60)),
        );
        let total = record.accumulated_cost_usd().unwrap();
        assert!((total - 1.00).abs() < 1e-9);
    }

    #[test]
    fn estimate_cost_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(estimate_cost("bug", dir.path()).is_none());
    }

    #[test]
    fn estimate_cost_no_matching_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let record = make_record("feature", RunStatus::Succeeded, Some(1.00));
        save_run(&record, dir.path()).unwrap();
        assert!(estimate_cost("bug", dir.path()).is_none());
    }

    #[test]
    fn estimate_cost_excludes_failed_runs() {
        let dir = tempfile::tempdir().unwrap();
        let record = make_record("bug", RunStatus::Failed, Some(0.50));
        save_run(&record, dir.path()).unwrap();
        assert!(estimate_cost("bug", dir.path()).is_none());
    }

    #[test]
    fn estimate_cost_excludes_runs_without_cost() {
        let dir = tempfile::tempdir().unwrap();
        let record = make_record("bug", RunStatus::Succeeded, None);
        save_run(&record, dir.path()).unwrap();
        assert!(estimate_cost("bug", dir.path()).is_none());
    }

    #[test]
    fn estimate_cost_single_run() {
        let dir = tempfile::tempdir().unwrap();
        let record = make_record("bug", RunStatus::Succeeded, Some(1.05));
        save_run(&record, dir.path()).unwrap();
        let est = estimate_cost("bug", dir.path()).unwrap();
        assert_eq!(est.count, 1);
        assert!((est.min - 1.05).abs() < 1e-9);
        assert!((est.max - 1.05).abs() < 1e-9);
        assert!((est.avg - 1.05).abs() < 1e-9);
        assert_eq!(est.workflow, "bug");
    }

    #[test]
    fn estimate_cost_multiple_runs() {
        let dir = tempfile::tempdir().unwrap();
        for cost in [0.70, 1.05, 1.50] {
            // Need unique run IDs — generate_run_id uses nanos so sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(2));
            let record = make_record("bug", RunStatus::Succeeded, Some(cost));
            save_run(&record, dir.path()).unwrap();
        }
        let est = estimate_cost("bug", dir.path()).unwrap();
        assert_eq!(est.count, 3);
        assert!((est.min - 0.70).abs() < 1e-9);
        assert!((est.max - 1.50).abs() < 1e-9);
        assert!((est.avg - 1.0833333333333333).abs() < 1e-9);
    }

    #[test]
    fn estimate_cost_mixed_workflows() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Succeeded, Some(0.80));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("feature", RunStatus::Succeeded, Some(2.00));
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record("bug", RunStatus::Succeeded, Some(1.20));
        save_run(&r3, dir.path()).unwrap();

        let est = estimate_cost("bug", dir.path()).unwrap();
        assert_eq!(est.count, 2);
        assert!((est.min - 0.80).abs() < 1e-9);
        assert!((est.max - 1.20).abs() < 1e-9);
        assert!((est.avg - 1.00).abs() < 1e-9);
    }

    #[test]
    fn hourly_cost_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(hourly_cost(dir.path()), 0.0);
    }

    #[test]
    fn hourly_cost_sums_recent_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Succeeded, Some(0.50));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("feature", RunStatus::Succeeded, Some(1.25));
        save_run(&r2, dir.path()).unwrap();
        let total = hourly_cost(dir.path());
        assert!((total - 1.75).abs() < 1e-9);
    }

    #[test]
    fn hourly_cost_excludes_old_runs() {
        let dir = tempfile::tempdir().unwrap();
        // Create a run with started_at more than 1 hour in the past.
        let mut old = make_record("bug", RunStatus::Succeeded, Some(5.00));
        old.started_at = chrono::Utc::now() - chrono::Duration::hours(2);
        save_run(&old, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let recent = make_record("bug", RunStatus::Succeeded, Some(0.30));
        save_run(&recent, dir.path()).unwrap();
        let total = hourly_cost(dir.path());
        assert!((total - 0.30).abs() < 1e-9);
    }

    #[test]
    fn hourly_cost_skips_runs_without_cost() {
        let dir = tempfile::tempdir().unwrap();
        let r = make_record("bug", RunStatus::Running, None);
        save_run(&r, dir.path()).unwrap();
        assert_eq!(hourly_cost(dir.path()), 0.0);
    }

    fn make_record_with_outcome(
        issue_number: u64,
        workflow: &str,
        outcome: Option<RouteOutcome>,
    ) -> RunRecord {
        let mut record = make_record(workflow, RunStatus::Succeeded, None);
        record.issue_number = issue_number;
        record.outcome = outcome;
        record
    }

    #[test]
    fn count_runs_for_subject_counts_all() {
        let dir = tempfile::tempdir().unwrap();
        let r1 =
            make_record_with_outcome(42, "pr-fix", Some(RouteOutcome::PrUpdated { number: 42 }));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record_with_outcome(
            42,
            "pr-fix",
            Some(RouteOutcome::Failed {
                stage: "implement".into(),
                error: "oops".into(),
            }),
        );
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record_with_outcome(42, "pr-fix", None);
        save_run(&r3, dir.path()).unwrap();
        assert_eq!(count_runs_for_subject(42, "pr-fix", dir.path()), 3);
    }

    #[test]
    fn count_failed_runs_for_subject_only_failed_and_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        // successful outcomes — should not count
        let r1 =
            make_record_with_outcome(10, "pr-fix", Some(RouteOutcome::PrUpdated { number: 10 }));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 =
            make_record_with_outcome(10, "pr-fix", Some(RouteOutcome::PrCreated { number: 11 }));
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record_with_outcome(10, "pr-fix", Some(RouteOutcome::NothingToDo));
        save_run(&r3, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        // failure outcomes — should count
        let r4 = make_record_with_outcome(
            10,
            "pr-fix",
            Some(RouteOutcome::Failed {
                stage: "review".into(),
                error: "bad".into(),
            }),
        );
        save_run(&r4, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r5 =
            make_record_with_outcome(10, "pr-fix", Some(RouteOutcome::Exhausted { retries: 3 }));
        save_run(&r5, dir.path()).unwrap();
        // no outcome — should not count
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r6 = make_record_with_outcome(10, "pr-fix", None);
        save_run(&r6, dir.path()).unwrap();
        assert_eq!(count_failed_runs_for_subject(10, "pr-fix", dir.path()), 2);
    }

    #[test]
    fn count_failed_runs_for_subject_filters_by_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let r1 = make_record_with_outcome(
            5,
            "pr-fix",
            Some(RouteOutcome::Failed {
                stage: "plan".into(),
                error: "err".into(),
            }),
        );
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record_with_outcome(
            5,
            "other-flow",
            Some(RouteOutcome::Failed {
                stage: "plan".into(),
                error: "err".into(),
            }),
        );
        save_run(&r2, dir.path()).unwrap();
        assert_eq!(count_failed_runs_for_subject(5, "pr-fix", dir.path()), 1);
        assert_eq!(
            count_failed_runs_for_subject(5, "other-flow", dir.path()),
            1
        );
    }

    #[test]
    fn count_failed_runs_for_subject_filters_by_issue() {
        let dir = tempfile::tempdir().unwrap();
        let r1 = make_record_with_outcome(
            1,
            "pr-fix",
            Some(RouteOutcome::Failed {
                stage: "test".into(),
                error: "fail".into(),
            }),
        );
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record_with_outcome(
            2,
            "pr-fix",
            Some(RouteOutcome::Failed {
                stage: "test".into(),
                error: "fail".into(),
            }),
        );
        save_run(&r2, dir.path()).unwrap();
        assert_eq!(count_failed_runs_for_subject(1, "pr-fix", dir.path()), 1);
        assert_eq!(count_failed_runs_for_subject(2, "pr-fix", dir.path()), 1);
    }

    #[test]
    fn summarize_by_workflow_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let summaries = summarize_by_workflow(dir.path());
        assert!(summaries.is_empty());
    }

    #[test]
    fn summarize_by_workflow_single_workflow_multiple_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Succeeded, Some(0.50));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("bug", RunStatus::Failed, Some(0.20));
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record("bug", RunStatus::Succeeded, Some(0.80));
        save_run(&r3, dir.path()).unwrap();

        let summaries = summarize_by_workflow(dir.path());
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.workflow, "bug");
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.succeeded, 2);
        assert_eq!(s.failed, 1);
        assert!((s.min_cost.unwrap() - 0.20).abs() < 1e-9);
        assert!((s.max_cost.unwrap() - 0.80).abs() < 1e-9);
        assert!((s.avg_cost.unwrap() - 0.50).abs() < 1e-9);
    }

    #[test]
    fn summarize_by_workflow_multiple_workflows_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("zebra", RunStatus::Succeeded, Some(1.00));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("alpha", RunStatus::Failed, Some(0.30));
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record("middle", RunStatus::Succeeded, Some(0.60));
        save_run(&r3, dir.path()).unwrap();

        let summaries = summarize_by_workflow(dir.path());
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].workflow, "alpha");
        assert_eq!(summaries[1].workflow, "middle");
        assert_eq!(summaries[2].workflow, "zebra");

        assert_eq!(summaries[0].total_runs, 1);
        assert_eq!(summaries[0].failed, 1);
        assert_eq!(summaries[0].succeeded, 0);
    }

    #[test]
    fn summarize_by_workflow_no_cost_data() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Succeeded, None);
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("bug", RunStatus::Failed, None);
        save_run(&r2, dir.path()).unwrap();

        let summaries = summarize_by_workflow(dir.path());
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.total_runs, 2);
        assert!(s.min_cost.is_none());
        assert!(s.max_cost.is_none());
        assert!(s.avg_cost.is_none());
    }

    #[test]
    fn summarize_by_workflow_mixed_cost_presence() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Succeeded, Some(1.00));
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("bug", RunStatus::Succeeded, None);
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record("bug", RunStatus::Succeeded, Some(3.00));
        save_run(&r3, dir.path()).unwrap();

        let summaries = summarize_by_workflow(dir.path());
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.succeeded, 3);
        // avg_cost computed only from costed runs: (1.00 + 3.00) / 2 = 2.00
        assert!((s.min_cost.unwrap() - 1.00).abs() < 1e-9);
        assert!((s.max_cost.unwrap() - 3.00).abs() < 1e-9);
        assert!((s.avg_cost.unwrap() - 2.00).abs() < 1e-9);
    }

    #[test]
    fn summarize_by_workflow_non_terminal_runs_count_but_not_succeeded_or_failed() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r1 = make_record("bug", RunStatus::Running, None);
        save_run(&r1, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = make_record("bug", RunStatus::Queued, None);
        save_run(&r2, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r3 = make_record("bug", RunStatus::Succeeded, Some(0.40));
        save_run(&r3, dir.path()).unwrap();

        let summaries = summarize_by_workflow(dir.path());
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.succeeded, 1);
        assert_eq!(s.failed, 0);
    }
}
