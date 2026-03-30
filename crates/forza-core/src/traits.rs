//! Trait definitions for forza's pluggable backends.
//!
//! These traits define the boundaries between forza-core and its implementations.
//! The core crate never depends on a specific GitHub client, git implementation,
//! or agent — it works through these traits, and the `forza` binary crate
//! provides the concrete implementations.

use std::path::Path;

use async_trait::async_trait;

use crate::error::Result;
use crate::run::StageResult;
use crate::subject::Subject;

/// Abstraction over GitHub API operations.
///
/// Implementations: `GhCliClient` (shells out to `gh`), `OctocrabClient` (REST API).
/// All methods take `&self` — implementations should be cheaply cloneable via `Arc`.
#[async_trait]
pub trait GitHubClient: Send + Sync {
    // ── Issues ──────────────────────────────────────────────────────

    /// Fetch a single issue by number.
    async fn fetch_issue(&self, repo: &str, number: u64) -> Result<Subject>;

    /// Fetch issues matching the given labels.
    async fn fetch_issues_with_labels(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<Subject>>;

    // ── Pull Requests ───────────────────────────────────────────────

    /// Fetch a single PR by number, with full mergeability and check status.
    async fn fetch_pr(&self, repo: &str, number: u64) -> Result<Subject>;

    /// Fetch all open PRs (for condition route evaluation).
    async fn fetch_all_open_prs(&self, repo: &str, limit: usize) -> Result<Vec<Subject>>;

    /// Fetch PRs matching the given labels.
    async fn fetch_prs_with_labels(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<Subject>>;

    /// Find a PR by its head branch name.
    async fn fetch_pr_by_branch(&self, repo: &str, branch: &str) -> Result<Option<Subject>>;

    // ── Mutations ───────────────────────────────────────────────────

    /// Add a label to an issue or PR.
    async fn add_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;

    /// Remove a label from an issue or PR.
    async fn remove_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;

    /// Create a label if it doesn't exist.
    async fn create_label(
        &self,
        repo: &str,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<()>;

    /// Post a comment on an issue or PR.
    async fn post_comment(&self, repo: &str, number: u64, body: &str) -> Result<()>;

    /// Create an issue. Returns the issue number.
    async fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<u64>;

    /// Create a pull request. Returns the PR number.
    async fn create_pr(
        &self,
        repo: &str,
        branch: &str,
        title: &str,
        body: &str,
        draft: bool,
        work_dir: &Path,
    ) -> Result<u64>;

    /// Update a PR's body text.
    async fn update_pr_body(&self, repo: &str, number: u64, body: &str) -> Result<()>;

    /// Mark a draft PR as ready for review.
    async fn mark_pr_ready(&self, repo: &str, number: u64) -> Result<()>;

    // ── Auth ────────────────────────────────────────────────────────

    /// Get the authenticated user's login.
    async fn authenticated_user(&self) -> Result<String>;
}

/// Abstraction over git operations.
///
/// Implementations: `GitCliClient` (shells out to `git`), `GixClient` (libgit2/gix).
#[async_trait]
pub trait GitClient: Send + Sync {
    /// Clone a repository.
    async fn clone_repo(&self, url: &str, dest: &Path) -> Result<()>;

    /// Fetch from the remote.
    async fn fetch(&self, repo_dir: &Path) -> Result<()>;

    /// Create a new branch from the current HEAD.
    async fn create_branch(&self, repo_dir: &Path, branch: &str) -> Result<()>;

    /// Checkout an existing branch.
    async fn checkout(&self, repo_dir: &Path, branch: &str) -> Result<()>;

    /// Push the current branch to the remote.
    async fn push(&self, repo_dir: &Path, branch: &str, force: bool) -> Result<()>;

    /// Create a git worktree.
    async fn create_worktree(
        &self,
        repo_dir: &Path,
        branch: &str,
        worktree_dir: &Path,
    ) -> Result<()>;

    /// Remove a git worktree.
    async fn remove_worktree(&self, repo_dir: &Path, worktree_dir: &Path) -> Result<()>;

    /// List existing worktrees.
    async fn list_worktrees(&self, repo_dir: &Path) -> Result<Vec<String>>;
}

/// Abstraction over agent execution (Claude, or any future LLM).
///
/// The core pipeline calls this trait to execute agent stages. The `forza`
/// binary crate provides `ClaudeAdapter` as the default implementation.
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    /// Resolve a model name for this agent.
    ///
    /// Returns `Some(model)` if the agent should use a specific model,
    /// or `None` to use the agent's built-in default. Implementations
    /// should filter out models belonging to other agents (e.g. Codex
    /// should ignore `claude-*` models and vice versa).
    fn resolve_model<'a>(&self, model: Option<&'a str>) -> Option<&'a str> {
        model
    }

    /// Execute a stage with the given prompt in the given working directory.
    ///
    /// Returns a `StageResult` with success/failure, duration, cost, and output.
    #[allow(clippy::too_many_arguments)]
    async fn execute(
        &self,
        stage_name: &str,
        prompt: &str,
        work_dir: &Path,
        model: Option<&str>,
        skills: &[String],
        mcp_config: Option<&str>,
        append_system_prompt: Option<&str>,
        allowed_tools: &[String],
    ) -> Result<StageResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify traits are object-safe (can be used as `dyn Trait`).
    fn _assert_github_object_safe(_: &dyn GitHubClient) {}
    fn _assert_git_object_safe(_: &dyn GitClient) {}
    fn _assert_agent_object_safe(_: &dyn AgentExecutor) {}
}
