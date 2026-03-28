//! GitHub client backed by the `gh` CLI.
//!
//! Delegates to the free functions in the parent module.
//! In a future refactor, the implementations will move here
//! and the free functions will be removed.

use std::path::Path;

use async_trait::async_trait;

use super::{GitHubClient, IssueCandidate, PrCandidate, PullRequest};
use crate::error::Result;

/// GitHub client that uses the `gh` CLI for all operations.
#[derive(Debug, Clone, Default)]
pub struct GhCliClient;

impl GhCliClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl GitHubClient for GhCliClient {
    async fn fetch_issue(&self, repo: &str, number: u64) -> Result<IssueCandidate> {
        super::fetch_issue(repo, number).await
    }

    async fn fetch_issue_state(&self, repo: &str, number: u64) -> Result<String> {
        super::fetch_issue_state(repo, number).await
    }

    async fn fetch_eligible_issues(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<IssueCandidate>> {
        super::fetch_eligible_issues(repo, labels, limit).await
    }

    async fn fetch_issues_with_label(
        &self,
        repo: &str,
        label: &str,
    ) -> Result<Vec<IssueCandidate>> {
        super::fetch_issues_with_label(repo, label).await
    }

    async fn add_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        super::add_label(repo, number, label).await
    }

    async fn remove_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        super::remove_label(repo, number, label).await
    }

    async fn create_label(
        &self,
        repo: &str,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<()> {
        super::create_label(repo, name, color, description).await
    }

    async fn comment_on_issue(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        super::comment_on_issue(repo, number, body).await
    }

    async fn close_issue(&self, repo: &str, number: u64) -> Result<()> {
        super::close_issue(repo, number).await
    }

    async fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<u64> {
        super::create_issue(repo, title, body).await
    }

    async fn fetch_pr(&self, repo: &str, number: u64) -> Result<PrCandidate> {
        super::fetch_pr(repo, number).await
    }

    async fn fetch_eligible_prs(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<PrCandidate>> {
        super::fetch_eligible_prs(repo, labels, limit).await
    }

    async fn fetch_prs_with_label(&self, repo: &str, label: &str) -> Result<Vec<PrCandidate>> {
        super::fetch_prs_with_label(repo, label).await
    }

    async fn fetch_all_open_prs(&self, repo: &str, limit: usize) -> Result<Vec<PrCandidate>> {
        super::fetch_all_open_prs(repo, limit).await
    }

    async fn fetch_pr_by_branch(&self, repo: &str, branch: &str) -> Result<Option<PrCandidate>> {
        super::fetch_pr_by_branch(repo, branch).await
    }

    async fn create_pull_request(
        &self,
        repo: &str,
        branch: &str,
        title: &str,
        body: &str,
        work_dir: &Path,
        draft: bool,
    ) -> Result<PullRequest> {
        super::create_pull_request(repo, branch, title, body, work_dir, draft).await
    }

    async fn mark_pr_ready_for_review(&self, repo: &str, number: u64) -> Result<()> {
        super::mark_pr_ready_for_review(repo, number).await
    }

    async fn update_pr_body(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        super::update_pr_body(repo, number, body).await
    }

    async fn add_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        super::add_pr_label(repo, number, label).await
    }

    async fn remove_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        super::remove_pr_label(repo, number, label).await
    }

    async fn comment_on_pr(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        super::comment_on_pr(repo, number, body).await
    }

    async fn fetch_authenticated_user(&self) -> Result<String> {
        super::fetch_authenticated_user().await
    }
}
