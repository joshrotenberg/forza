//! Adapters that bridge existing implementations to forza-core traits.
//!
//! These thin wrappers let us use the existing `GhCliClient`, `OctocrabClient`,
//! `GitCliClient`/`GixClient`, and `ClaudeAdapter` through the forza-core
//! trait interfaces.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use forza_core::error::{Error as CoreError, Result as CoreResult};
use forza_core::run::StageResult as CoreStageResult;
use forza_core::subject::{Subject, SubjectKind};
use tracing::warn;

// ── Shared helpers ─────────────────────────────────────────────────────

/// Read skill files and prepend their contents to a prompt.
///
/// Each skill path is resolved relative to `work_dir` if not absolute.
/// Unreadable files are skipped with a warning.
pub fn prepend_skill_files(prompt: &str, skills: &[String], work_dir: &Path) -> String {
    if skills.is_empty() {
        return prompt.to_string();
    }
    let mut parts = Vec::with_capacity(skills.len() + 1);
    for skill in skills {
        let skill_path = {
            let p = Path::new(skill);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                work_dir.join(p)
            }
        };
        match std::fs::read_to_string(&skill_path) {
            Ok(content) => parts.push(content),
            Err(e) => {
                warn!(path = %skill_path.display(), error = %e, "skipping unreadable skill file");
            }
        }
    }
    parts.push(prompt.to_string());
    parts.join("\n\n")
}

use crate::github::{IssueCandidate, PrCandidate};

// ── Type conversions ────────────────────────────────────────────────────

/// Convert an `IssueCandidate` to a `Subject`.
pub fn issue_to_subject(issue: &IssueCandidate, branch: &str) -> Subject {
    Subject {
        kind: SubjectKind::Issue,
        number: issue.number,
        repo: issue.repo.clone(),
        title: issue.title.clone(),
        body: issue.body.clone(),
        labels: issue.labels.clone(),
        html_url: issue.html_url.clone(),
        author: issue.author.clone(),
        branch: branch.to_string(),
        comments: issue.comments.clone(),
        mergeable: None,
        checks_passing: None,
        review_decision: None,
        is_draft: None,
        base_branch: None,
    }
}

/// Convert a `PrCandidate` to a `Subject`.
pub fn pr_to_subject(pr: &PrCandidate) -> Subject {
    Subject {
        kind: SubjectKind::Pr,
        number: pr.number,
        repo: pr.repo.clone(),
        title: pr.title.clone(),
        body: pr.body.clone(),
        labels: pr.labels.clone(),
        html_url: pr.html_url.clone(),
        author: String::new(),
        branch: pr.head_branch.clone(),
        comments: Vec::new(),
        mergeable: pr.mergeable.clone(),
        checks_passing: pr.checks_passing,
        review_decision: pr.review_decision.clone(),
        is_draft: Some(pr.is_draft),
        base_branch: Some(pr.base_branch.clone()),
    }
}

// ── GitHub adapter ──────────────────────────────────────────────────────

/// Wraps an existing `GitHubClient` implementation to satisfy `forza_core::GitHubClient`.
pub struct GitHubAdapter {
    inner: Arc<dyn crate::github::GitHubClient>,
}

impl GitHubAdapter {
    pub fn new(inner: Arc<dyn crate::github::GitHubClient>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl forza_core::GitHubClient for GitHubAdapter {
    async fn fetch_issue(&self, repo: &str, number: u64) -> CoreResult<Subject> {
        let issue = self
            .inner
            .fetch_issue(repo, number)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(issue_to_subject(&issue, ""))
    }

    async fn fetch_issues_with_labels(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> CoreResult<Vec<Subject>> {
        let issues = self
            .inner
            .fetch_eligible_issues(repo, labels, limit)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(issues.iter().map(|i| issue_to_subject(i, "")).collect())
    }

    async fn fetch_pr(&self, repo: &str, number: u64) -> CoreResult<Subject> {
        let pr = self
            .inner
            .fetch_pr(repo, number)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(pr_to_subject(&pr))
    }

    async fn fetch_all_open_prs(&self, repo: &str, limit: usize) -> CoreResult<Vec<Subject>> {
        let prs = self
            .inner
            .fetch_all_open_prs(repo, limit)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(prs.iter().map(pr_to_subject).collect())
    }

    async fn fetch_prs_with_labels(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> CoreResult<Vec<Subject>> {
        let prs = self
            .inner
            .fetch_eligible_prs(repo, labels, limit)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(prs.iter().map(pr_to_subject).collect())
    }

    async fn fetch_pr_by_branch(&self, repo: &str, branch: &str) -> CoreResult<Option<Subject>> {
        let pr = self
            .inner
            .fetch_pr_by_branch(repo, branch)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(pr.as_ref().map(pr_to_subject))
    }

    async fn add_label(&self, repo: &str, number: u64, label: &str) -> CoreResult<()> {
        self.inner
            .add_label(repo, number, label)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn remove_label(&self, repo: &str, number: u64, label: &str) -> CoreResult<()> {
        self.inner
            .remove_label(repo, number, label)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn create_label(
        &self,
        repo: &str,
        name: &str,
        color: &str,
        description: &str,
    ) -> CoreResult<()> {
        self.inner
            .create_label(repo, name, color, description)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn post_comment(&self, repo: &str, number: u64, body: &str) -> CoreResult<()> {
        self.inner
            .comment_on_issue(repo, number, body)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn create_issue(&self, repo: &str, title: &str, body: &str) -> CoreResult<u64> {
        self.inner
            .create_issue(repo, title, body)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn create_pr(
        &self,
        repo: &str,
        branch: &str,
        title: &str,
        body: &str,
        draft: bool,
        work_dir: &Path,
    ) -> CoreResult<u64> {
        let pr = self
            .inner
            .create_pull_request(repo, branch, title, body, work_dir, draft)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))?;
        Ok(pr.number)
    }

    async fn update_pr_body(&self, repo: &str, number: u64, body: &str) -> CoreResult<()> {
        self.inner
            .update_pr_body(repo, number, body)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn mark_pr_ready(&self, repo: &str, number: u64) -> CoreResult<()> {
        self.inner
            .mark_pr_ready_for_review(repo, number)
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }

    async fn authenticated_user(&self) -> CoreResult<String> {
        self.inner
            .fetch_authenticated_user()
            .await
            .map_err(|e| CoreError::GitHub(e.to_string()))
    }
}

// ── Git adapter ─────────────────────────────────────────────────────────

/// Wraps an existing `GitClient` implementation to satisfy `forza_core::GitClient`.
pub struct GitAdapter {
    inner: Arc<dyn crate::git::GitClient>,
}

impl GitAdapter {
    pub fn new(inner: Arc<dyn crate::git::GitClient>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl forza_core::GitClient for GitAdapter {
    async fn clone_repo(&self, _url: &str, _dest: &Path) -> CoreResult<()> {
        // Not used in the current flow — repos are pre-cloned or found via repo_dir.
        Err(CoreError::Git("clone not implemented via adapter".into()))
    }

    async fn fetch(&self, repo_dir: &Path) -> CoreResult<()> {
        self.inner
            .fetch(repo_dir)
            .await
            .map_err(|e| CoreError::Git(e.to_string()))
    }

    async fn create_branch(&self, _repo_dir: &Path, _branch: &str) -> CoreResult<()> {
        // Branch creation is handled by worktree_add in the old API.
        Ok(())
    }

    async fn checkout(&self, _repo_dir: &Path, _branch: &str) -> CoreResult<()> {
        // Checkout is handled by worktree_add in the old API.
        Ok(())
    }

    async fn push(&self, repo_dir: &Path, branch: &str, force: bool) -> CoreResult<()> {
        if force {
            self.inner
                .push_force(repo_dir, branch)
                .await
                .map_err(|e| CoreError::Git(e.to_string()))
        } else {
            self.inner
                .push(repo_dir, branch)
                .await
                .map_err(|e| CoreError::Git(e.to_string()))
        }
    }

    async fn create_worktree(
        &self,
        repo_dir: &Path,
        branch: &str,
        _worktree_dir: &Path,
    ) -> CoreResult<()> {
        // Delegate to the old worktree_add which handles branch creation + checkout.
        self.inner
            .worktree_add(repo_dir, branch, ".worktrees")
            .await
            .map(|_| ())
            .map_err(|e| CoreError::Isolation(e.to_string()))
    }

    async fn remove_worktree(&self, repo_dir: &Path, worktree_dir: &Path) -> CoreResult<()> {
        self.inner
            .worktree_remove(repo_dir, worktree_dir, true)
            .await
            .map_err(|e| CoreError::Isolation(e.to_string()))
    }

    async fn list_worktrees(&self, _repo_dir: &Path) -> CoreResult<Vec<String>> {
        // Not directly available in old API. Return empty for now.
        Ok(vec![])
    }
}

// ── Agent factory ──────────────────────────────────────────────────────

/// Create the appropriate agent executor based on the agent name.
///
/// Supported values: `"claude"` (default), `"codex"`.
pub fn create_agent(agent_name: &str) -> Arc<dyn forza_core::AgentExecutor> {
    match agent_name {
        "codex" => {
            tracing::info!(agent = "codex", "using Codex agent backend");
            Arc::new(CodexAgentAdapter)
        }
        other => {
            if other != "claude" {
                tracing::warn!(agent = other, "unknown agent, falling back to Claude");
            }
            tracing::info!(agent = "claude", "using Claude agent backend");
            Arc::new(ClaudeAgentAdapter)
        }
    }
}

// ── Agent adapter ───────────────────────────────────────────────────────

/// Wraps `claude-wrapper` to satisfy `forza_core::AgentExecutor`.
pub struct ClaudeAgentAdapter;

#[async_trait]
impl forza_core::AgentExecutor for ClaudeAgentAdapter {
    fn resolve_model<'a>(&self, model: Option<&'a str>) -> Option<&'a str> {
        match model {
            // Accept claude models and unrecognized models (might be aliases like "sonnet").
            Some(m) if m.starts_with("o1") || m.starts_with("o3") || m.starts_with("gpt") => {
                tracing::warn!(model = m, "ignoring non-Claude model for Claude agent");
                None
            }
            other => other,
        }
    }

    async fn execute(
        &self,
        stage_name: &str,
        prompt: &str,
        work_dir: &Path,
        model: Option<&str>,
        max_budget_usd: Option<f64>,
        skills: &[String],
        mcp_config: Option<&str>,
        append_system_prompt: Option<&str>,
        allowed_tools: &[String],
    ) -> CoreResult<CoreStageResult> {
        let mut adapter = crate::executor::ClaudeAdapter::new();
        if let Some(m) = model {
            adapter = adapter.model(m);
        }
        if let Some(budget) = max_budget_usd {
            adapter = adapter.max_budget_usd(budget);
        }
        if !skills.is_empty() {
            adapter = adapter.skills(skills.iter().cloned());
        }
        if let Some(p) = mcp_config {
            adapter = adapter.mcp_config(p);
        }
        if let Some(s) = append_system_prompt {
            adapter = adapter.append_system_prompt(s);
        }

        // Create a minimal PlannedStage for the existing executor interface.
        let planned = crate::planner::PlannedStage {
            kind: crate::workflow::StageKind::from_name(stage_name),
            prompt: prompt.to_string(),
            allowed_files: None,
            validation: vec![],
            optional: false,
            max_retries: 0,
            condition: None,
            agentless: false,
            command: None,
            model: None,
            skills: None,
            mcp_config: None,
            allowed_tools: allowed_tools.to_vec(),
        };

        let result = crate::executor::AgentAdapter::execute_stage(&adapter, &planned, work_dir)
            .await
            .map_err(|e| CoreError::Agent(e.to_string()))?;

        Ok(CoreStageResult {
            stage: "agent".into(),
            success: result.success,
            duration_secs: result.duration_secs,
            cost_usd: result.cost_usd,
            output: result.output,
            files_modified: None,
        })
    }
}

// ── Codex agent adapter ─────────────────────────────────────────────

/// Wraps `codex-wrapper` to satisfy `forza_core::AgentExecutor`.
///
/// Uses `codex exec --dangerously-bypass-approvals-and-sandbox` so Codex has
/// full git access in the worktree (the default `--full-auto` sandbox makes
/// `.git` read-only, preventing commits). Codex uses its own default model
/// unless overridden.
pub struct CodexAgentAdapter;

#[async_trait]
impl forza_core::AgentExecutor for CodexAgentAdapter {
    fn resolve_model<'a>(&self, model: Option<&'a str>) -> Option<&'a str> {
        match model {
            Some(m)
                if m.starts_with("claude")
                    || m.starts_with("sonnet")
                    || m.starts_with("opus")
                    || m.starts_with("haiku") =>
            {
                tracing::warn!(model = m, "ignoring Claude model for Codex agent");
                None
            }
            other => other,
        }
    }

    async fn execute(
        &self,
        stage_name: &str,
        prompt: &str,
        work_dir: &Path,
        model: Option<&str>,
        max_budget_usd: Option<f64>,
        skills: &[String],
        mcp_config: Option<&str>,
        append_system_prompt: Option<&str>,
        allowed_tools: &[String],
    ) -> CoreResult<CoreStageResult> {
        if max_budget_usd.is_some() {
            tracing::warn!(
                stage = stage_name,
                "Codex does not support max_budget_usd; value will be ignored"
            );
        }
        if mcp_config.is_some() {
            tracing::warn!(
                stage = stage_name,
                "Codex does not support MCP config; mcp_config will be ignored"
            );
        }
        if append_system_prompt.is_some() {
            tracing::warn!(
                stage = stage_name,
                "Codex does not support append_system_prompt; value will be ignored"
            );
        }
        if !allowed_tools.is_empty() {
            tracing::warn!(
                stage = stage_name,
                "Codex does not support allowed_tools; {} tool(s) will be ignored",
                allowed_tools.len()
            );
        }

        let full_prompt = prepend_skill_files(prompt, skills, work_dir);

        let codex = codex_wrapper::Codex::builder()
            .working_dir(work_dir)
            .build()
            .map_err(|e| CoreError::Agent(format!("failed to create codex client: {e}")))?;

        // Use ReviewCommand for review stages (codex exec review), ExecCommand
        // for everything else. Both are ephemeral since forza stages are one-shot.
        let start = std::time::Instant::now();
        let result = if stage_name == "review" {
            let mut cmd = codex_wrapper::ReviewCommand::new()
                .prompt(&full_prompt)
                .dangerously_bypass_approvals_and_sandbox()
                .ephemeral();
            if let Some(m) = model {
                cmd = cmd.model(m);
            }
            codex_wrapper::command::CodexCommand::execute(&cmd, &codex).await
        } else {
            let mut cmd = codex_wrapper::ExecCommand::new(&full_prompt)
                .dangerously_bypass_approvals_and_sandbox()
                .ephemeral();
            if let Some(m) = model {
                cmd = cmd.model(m);
            }
            codex_wrapper::command::CodexCommand::execute(&cmd, &codex).await
        };
        let duration = start.elapsed();

        match result {
            Ok(output) => Ok(CoreStageResult {
                stage: "codex".into(),
                success: output.success,
                duration_secs: duration.as_secs_f64(),
                cost_usd: None,
                output: if output.stdout.is_empty() {
                    output.stderr
                } else {
                    output.stdout
                },
                files_modified: None,
            }),
            Err(e) => {
                let error_msg = match &e {
                    codex_wrapper::Error::CommandFailed { stdout, stderr, .. } => {
                        if stderr.is_empty() {
                            stdout.clone()
                        } else {
                            stderr.clone()
                        }
                    }
                    other => other.to_string(),
                };
                Ok(CoreStageResult {
                    stage: "codex".into(),
                    success: false,
                    duration_secs: duration.as_secs_f64(),
                    cost_usd: None,
                    output: error_msg,
                    files_modified: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_to_subject_converts() {
        let issue = IssueCandidate {
            number: 42,
            repo: "owner/repo".into(),
            title: "Fix bug".into(),
            body: "It's broken".into(),
            labels: vec!["bug".into()],
            state: "open".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            is_assigned: false,
            html_url: "https://github.com/owner/repo/issues/42".into(),
            author: "user".into(),
            comments: vec![],
        };
        let subject = issue_to_subject(&issue, "automation/42-fix");
        assert_eq!(subject.kind, SubjectKind::Issue);
        assert_eq!(subject.number, 42);
        assert_eq!(subject.branch, "automation/42-fix");
        assert!(subject.mergeable.is_none());
    }

    #[test]
    fn pr_to_subject_converts() {
        let pr = PrCandidate {
            number: 99,
            repo: "owner/repo".into(),
            title: "fix: bug".into(),
            body: "Fixes #42".into(),
            labels: vec![],
            state: "open".into(),
            html_url: "https://github.com/owner/repo/pull/99".into(),
            head_branch: "fix/bug".into(),
            base_branch: "main".into(),
            is_draft: false,
            mergeable: Some("MERGEABLE".into()),
            review_decision: Some("APPROVED".into()),
            checks_passing: Some(true),
        };
        let subject = pr_to_subject(&pr);
        assert_eq!(subject.kind, SubjectKind::Pr);
        assert_eq!(subject.number, 99);
        assert_eq!(subject.branch, "fix/bug");
        assert_eq!(subject.mergeable.as_deref(), Some("MERGEABLE"));
        assert_eq!(subject.checks_passing, Some(true));
        assert_eq!(subject.base_branch.as_deref(), Some("main"));
    }
}
