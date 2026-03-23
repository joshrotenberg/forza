//! Mock implementations of forza-core traits for testing.
//!
//! Enabled by the `testing` feature flag. Provides configurable mocks for
//! `GitHubClient`, `GitClient`, and `AgentExecutor` that run instantly
//! and track all calls for verification.
//!
//! ```rust,ignore
//! use forza_core::testing::*;
//!
//! let gh = MockGitHub::new()
//!     .with_issue(42, "Fix bug", &["bug", "forza:ready"]);
//! let git = MockGit::new();
//! let agent = MockAgent::new()
//!     .on_any(success_result());
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::{Error, Result};
use crate::run::StageResult;
use crate::subject::{Subject, SubjectKind};

// ── Helpers ─────────────────────────────────────────────────────────────

/// Create a successful `StageResult`.
pub fn success_result() -> StageResult {
    StageResult {
        stage: String::new(),
        success: true,
        duration_secs: 0.1,
        cost_usd: Some(0.01),
        output: "ok".into(),
        files_modified: None,
    }
}

/// Create a failed `StageResult`.
pub fn failure_result(reason: &str) -> StageResult {
    StageResult {
        stage: String::new(),
        success: false,
        duration_secs: 0.1,
        cost_usd: None,
        output: reason.into(),
        files_modified: None,
    }
}

/// Create a test `Subject` for an issue.
pub fn make_test_issue(number: u64, title: &str, labels: &[&str]) -> Subject {
    Subject {
        kind: SubjectKind::Issue,
        number,
        repo: "test/repo".into(),
        title: title.into(),
        body: format!("Body of issue #{number}"),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        html_url: format!("https://github.com/test/repo/issues/{number}"),
        author: "testuser".into(),
        branch: format!("automation/{number}-{}", slugify(title)),
        mergeable: None,
        checks_passing: None,
        review_decision: None,
        is_draft: None,
        base_branch: None,
    }
}

/// Create a test `Subject` for a PR.
pub fn make_test_pr(
    number: u64,
    title: &str,
    branch: &str,
    mergeable: Option<&str>,
    checks_passing: Option<bool>,
    review_decision: Option<&str>,
) -> Subject {
    Subject {
        kind: SubjectKind::Pr,
        number,
        repo: "test/repo".into(),
        title: title.into(),
        body: format!("Body of PR #{number}"),
        labels: vec![],
        html_url: format!("https://github.com/test/repo/pull/{number}"),
        author: "testuser".into(),
        branch: branch.into(),
        mergeable: mergeable.map(String::from),
        checks_passing,
        review_decision: review_decision.map(String::from),
        is_draft: Some(false),
        base_branch: Some("main".into()),
    }
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ── Call tracking ───────────────────────────────────────────────────────

/// A recorded call to a mock, for verification.
#[derive(Debug, Clone)]
pub enum MockCall {
    FetchIssue(u64),
    FetchPr(u64),
    FetchAllOpenPrs,
    AddLabel(u64, String),
    RemoveLabel(u64, String),
    PostComment(u64, String),
    CreateIssue(String), // title
    CreatePr(String),    // branch
    UpdatePrBody(u64),
    MarkPrReady(u64),
    AgentExecute(String),   // prompt (truncated)
    CreateWorktree(String), // branch
    RemoveWorktree,
}

// ── MockGitHub ──────────────────────────────────────────────────────────

/// Mock GitHub client. Configure with issues/PRs, tracks all calls.
pub struct MockGitHub {
    issues: HashMap<u64, Subject>,
    prs: HashMap<u64, Subject>,
    prs_by_branch: HashMap<String, u64>,
    all_open_prs: Vec<Subject>,
    next_pr_number: Arc<Mutex<u64>>,
    next_issue_number: Arc<Mutex<u64>>,
    pub calls: Arc<Mutex<Vec<MockCall>>>,
}

impl MockGitHub {
    pub fn new() -> Self {
        Self {
            issues: HashMap::new(),
            prs: HashMap::new(),
            prs_by_branch: HashMap::new(),
            all_open_prs: Vec::new(),
            next_pr_number: Arc::new(Mutex::new(100)),
            next_issue_number: Arc::new(Mutex::new(200)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add an issue that can be fetched.
    pub fn with_issue(mut self, number: u64, title: &str, labels: &[&str]) -> Self {
        self.issues
            .insert(number, make_test_issue(number, title, labels));
        self
    }

    /// Add a PR that can be fetched.
    pub fn with_pr(
        mut self,
        number: u64,
        title: &str,
        branch: &str,
        mergeable: Option<&str>,
        checks_passing: Option<bool>,
    ) -> Self {
        let pr = make_test_pr(number, title, branch, mergeable, checks_passing, None);
        self.prs.insert(number, pr.clone());
        self.prs_by_branch.insert(branch.to_string(), number);
        self.all_open_prs.push(pr);
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Check if a specific label was added to a subject.
    pub fn label_was_added(&self, number: u64, label: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, MockCall::AddLabel(n, l) if *n == number && l == label))
    }

    /// Check if a specific label was removed from a subject.
    pub fn label_was_removed(&self, number: u64, label: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, MockCall::RemoveLabel(n, l) if *n == number && l == label))
    }

    /// Check if an issue was created.
    pub fn issue_was_created(&self) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, MockCall::CreateIssue(_)))
    }

    /// Check if a PR was created.
    pub fn pr_was_created(&self) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, MockCall::CreatePr(_)))
    }

    /// Check if a comment was posted.
    pub fn comment_was_posted(&self, number: u64) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, MockCall::PostComment(n, _) if *n == number))
    }

    fn record(&self, call: MockCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl Default for MockGitHub {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::GitHubClient for MockGitHub {
    async fn fetch_issue(&self, _repo: &str, number: u64) -> Result<Subject> {
        self.record(MockCall::FetchIssue(number));
        self.issues
            .get(&number)
            .cloned()
            .ok_or_else(|| Error::GitHub(format!("issue #{number} not found")))
    }

    async fn fetch_issues_with_labels(
        &self,
        _repo: &str,
        labels: &[String],
        _limit: usize,
    ) -> Result<Vec<Subject>> {
        Ok(self
            .issues
            .values()
            .filter(|i| labels.iter().all(|l| i.labels.contains(l)))
            .cloned()
            .collect())
    }

    async fn fetch_pr(&self, _repo: &str, number: u64) -> Result<Subject> {
        self.record(MockCall::FetchPr(number));
        self.prs
            .get(&number)
            .cloned()
            .ok_or_else(|| Error::GitHub(format!("PR #{number} not found")))
    }

    async fn fetch_all_open_prs(&self, _repo: &str, _limit: usize) -> Result<Vec<Subject>> {
        self.record(MockCall::FetchAllOpenPrs);
        Ok(self.all_open_prs.clone())
    }

    async fn fetch_prs_with_labels(
        &self,
        _repo: &str,
        labels: &[String],
        _limit: usize,
    ) -> Result<Vec<Subject>> {
        Ok(self
            .all_open_prs
            .iter()
            .filter(|pr| labels.iter().all(|l| pr.labels.contains(l)))
            .cloned()
            .collect())
    }

    async fn fetch_pr_by_branch(&self, _repo: &str, branch: &str) -> Result<Option<Subject>> {
        Ok(self
            .prs_by_branch
            .get(branch)
            .and_then(|n| self.prs.get(n))
            .cloned())
    }

    async fn create_issue(&self, _repo: &str, title: &str, _body: &str) -> Result<u64> {
        self.record(MockCall::CreateIssue(title.to_string()));
        let mut next = self.next_issue_number.lock().unwrap();
        let number = *next;
        *next += 1;
        Ok(number)
    }

    async fn add_label(&self, _repo: &str, number: u64, label: &str) -> Result<()> {
        self.record(MockCall::AddLabel(number, label.to_string()));
        Ok(())
    }

    async fn remove_label(&self, _repo: &str, number: u64, label: &str) -> Result<()> {
        self.record(MockCall::RemoveLabel(number, label.to_string()));
        Ok(())
    }

    async fn create_label(
        &self,
        _repo: &str,
        _name: &str,
        _color: &str,
        _description: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn post_comment(&self, _repo: &str, number: u64, body: &str) -> Result<()> {
        self.record(MockCall::PostComment(number, body.to_string()));
        Ok(())
    }

    async fn create_pr(
        &self,
        _repo: &str,
        branch: &str,
        _title: &str,
        _body: &str,
        _draft: bool,
        _work_dir: &Path,
    ) -> Result<u64> {
        self.record(MockCall::CreatePr(branch.to_string()));
        let mut next = self.next_pr_number.lock().unwrap();
        let number = *next;
        *next += 1;
        Ok(number)
    }

    async fn update_pr_body(&self, _repo: &str, number: u64, _body: &str) -> Result<()> {
        self.record(MockCall::UpdatePrBody(number));
        Ok(())
    }

    async fn mark_pr_ready(&self, _repo: &str, number: u64) -> Result<()> {
        self.record(MockCall::MarkPrReady(number));
        Ok(())
    }

    async fn authenticated_user(&self) -> Result<String> {
        Ok("testuser".into())
    }
}

// ── MockGit ─────────────────────────────────────────────────────────────

/// Mock git client. Tracks worktree operations.
pub struct MockGit {
    pub calls: Arc<Mutex<Vec<MockCall>>>,
    fail_worktree: bool,
}

impl MockGit {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_worktree: false,
        }
    }

    /// Make worktree creation fail.
    pub fn fail_worktree(mut self) -> Self {
        self.fail_worktree = true;
        self
    }

    fn record(&self, call: MockCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl Default for MockGit {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::GitClient for MockGit {
    async fn clone_repo(&self, _url: &str, _dest: &Path) -> Result<()> {
        Ok(())
    }

    async fn fetch(&self, _repo_dir: &Path) -> Result<()> {
        Ok(())
    }

    async fn create_branch(&self, _repo_dir: &Path, _branch: &str) -> Result<()> {
        Ok(())
    }

    async fn checkout(&self, _repo_dir: &Path, _branch: &str) -> Result<()> {
        Ok(())
    }

    async fn push(&self, _repo_dir: &Path, _branch: &str, _force: bool) -> Result<()> {
        Ok(())
    }

    async fn create_worktree(
        &self,
        _repo_dir: &Path,
        branch: &str,
        worktree_dir: &Path,
    ) -> Result<()> {
        self.record(MockCall::CreateWorktree(branch.to_string()));
        if self.fail_worktree {
            Err(Error::Isolation("mock worktree failure".into()))
        } else {
            // Create the directory so shell commands can run in it.
            std::fs::create_dir_all(worktree_dir).ok();
            Ok(())
        }
    }

    async fn remove_worktree(&self, _repo_dir: &Path, _worktree_dir: &Path) -> Result<()> {
        self.record(MockCall::RemoveWorktree);
        Ok(())
    }

    async fn list_worktrees(&self, _repo_dir: &Path) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

// ── MockAgent ───────────────────────────────────────────────────────────

/// Mock agent executor. Returns configurable results per stage.
pub struct MockAgent {
    stage_results: HashMap<String, StageResult>,
    default_result: StageResult,
    pub calls: Arc<Mutex<Vec<String>>>,
}

impl MockAgent {
    pub fn new() -> Self {
        Self {
            stage_results: HashMap::new(),
            default_result: success_result(),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the result for a specific stage (matched by prompt content).
    pub fn on_prompt_containing(mut self, needle: &str, result: StageResult) -> Self {
        self.stage_results.insert(needle.to_string(), result);
        self
    }

    /// Set the default result for all stages.
    pub fn default_result(mut self, result: StageResult) -> Self {
        self.default_result = result;
        self
    }

    /// Make all stages fail.
    pub fn always_fail(self, reason: &str) -> Self {
        self.default_result(failure_result(reason))
    }

    /// Get all prompts that were executed.
    pub fn executed_prompts(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// How many stages were executed.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Default for MockAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::traits::AgentExecutor for MockAgent {
    async fn execute(
        &self,
        _stage_name: &str,
        prompt: &str,
        _work_dir: &Path,
        _model: Option<&str>,
        _skills: &[String],
        _mcp_config: Option<&str>,
        _append_system_prompt: Option<&str>,
        _allowed_tools: &[String],
    ) -> Result<StageResult> {
        let truncated = if prompt.len() > 100 {
            format!("{}...", &prompt[..100])
        } else {
            prompt.to_string()
        };
        self.calls.lock().unwrap().push(truncated);

        // Check for stage-specific results.
        for (needle, result) in &self.stage_results {
            if prompt.contains(needle) {
                return Ok(result.clone());
            }
        }

        Ok(self.default_result.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{AgentExecutor, GitClient, GitHubClient};

    #[test]
    fn make_test_issue_has_correct_fields() {
        let issue = make_test_issue(42, "Fix bug", &["bug", "forza:ready"]);
        assert_eq!(issue.kind, SubjectKind::Issue);
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Fix bug");
        assert!(issue.has_label("bug"));
        assert!(issue.has_label("forza:ready"));
        assert!(issue.branch.starts_with("automation/42-"));
    }

    #[test]
    fn make_test_pr_has_correct_fields() {
        let pr = make_test_pr(
            99,
            "fix: thing",
            "automation/99-fix",
            Some("MERGEABLE"),
            Some(true),
            None,
        );
        assert_eq!(pr.kind, SubjectKind::Pr);
        assert_eq!(pr.number, 99);
        assert_eq!(pr.mergeable.as_deref(), Some("MERGEABLE"));
        assert_eq!(pr.checks_passing, Some(true));
    }

    #[tokio::test]
    async fn mock_github_fetch_issue() {
        let gh = MockGitHub::new().with_issue(42, "Fix bug", &["bug"]);
        let issue = gh.fetch_issue("test/repo", 42).await.unwrap();
        assert_eq!(issue.number, 42);
    }

    #[tokio::test]
    async fn mock_github_fetch_missing_issue() {
        let gh = MockGitHub::new();
        let result = gh.fetch_issue("test/repo", 999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_github_tracks_labels() {
        let gh = MockGitHub::new();
        gh.add_label("test/repo", 42, "forza:in-progress")
            .await
            .unwrap();
        gh.remove_label("test/repo", 42, "forza:in-progress")
            .await
            .unwrap();
        gh.add_label("test/repo", 42, "forza:complete")
            .await
            .unwrap();

        assert!(gh.label_was_added(42, "forza:in-progress"));
        assert!(gh.label_was_removed(42, "forza:in-progress"));
        assert!(gh.label_was_added(42, "forza:complete"));
        assert!(!gh.label_was_added(42, "forza:failed"));
    }

    #[tokio::test]
    async fn mock_github_creates_pr() {
        let gh = MockGitHub::new();
        let number = gh
            .create_pr(
                "test/repo",
                "automation/42-fix",
                "fix",
                "body",
                false,
                Path::new("/tmp"),
            )
            .await
            .unwrap();
        assert_eq!(number, 100);
        assert!(gh.pr_was_created());
    }

    #[tokio::test]
    async fn mock_agent_default_succeeds() {
        let agent = MockAgent::new();
        let result = agent
            .execute(
                "test",
                "do the thing",
                Path::new("/tmp"),
                None,
                &[],
                None,
                None,
                &[],
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(agent.call_count(), 1);
    }

    #[tokio::test]
    async fn mock_agent_per_stage_results() {
        let agent = MockAgent::new()
            .on_prompt_containing("plan", success_result())
            .on_prompt_containing("implement", failure_result("compile error"));

        let plan = agent
            .execute(
                "plan",
                "create a plan",
                Path::new("/tmp"),
                None,
                &[],
                None,
                None,
                &[],
            )
            .await
            .unwrap();
        assert!(plan.success);

        let implement = agent
            .execute(
                "implement",
                "implement the fix",
                Path::new("/tmp"),
                None,
                &[],
                None,
                None,
                &[],
            )
            .await
            .unwrap();
        assert!(!implement.success);
        assert_eq!(implement.output, "compile error");
    }

    #[tokio::test]
    async fn mock_agent_always_fail() {
        let agent = MockAgent::new().always_fail("nope");
        let result = agent
            .execute(
                "test",
                "anything",
                Path::new("/tmp"),
                None,
                &[],
                None,
                None,
                &[],
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.output, "nope");
    }

    #[tokio::test]
    async fn mock_git_worktree_success() {
        let git = MockGit::new();
        git.create_worktree(Path::new("/repo"), "fix/thing", Path::new("/wt"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn mock_git_worktree_failure() {
        let git = MockGit::new().fail_worktree();
        let result = git
            .create_worktree(Path::new("/repo"), "fix/thing", Path::new("/wt"))
            .await;
        assert!(result.is_err());
    }
}
