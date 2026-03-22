//! GitHub client backed by the octocrab crate (REST API).

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use http::{
    Uri,
    header::{AUTHORIZATION, USER_AGENT},
};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use octocrab::{
    AuthState, Octocrab, OctocrabBuilder,
    service::middleware::{base_uri::BaseUriLayer, extra_headers::ExtraHeadersLayer},
};

use super::{GitHubClient, IssueCandidate, PrCandidate, PullRequest};
use crate::error::{Error, Result};

/// GitHub client using the octocrab crate for REST API access.
pub struct OctocrabClient {
    client: Octocrab,
}

impl OctocrabClient {
    /// Create a new client using the GITHUB_TOKEN env var.
    /// Falls back to `gh auth token` if GITHUB_TOKEN is not set.
    pub async fn new() -> Result<Self> {
        let token = if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            token
        } else {
            let output = tokio::process::Command::new("gh")
                .args(["auth", "token"])
                .output()
                .await
                .map_err(|e| Error::GitHub(format!("failed to get gh auth token: {e}")))?;
            if !output.status.success() {
                return Err(Error::GitHub(
                    "GITHUB_TOKEN not set and `gh auth token` failed".into(),
                ));
            }
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };

        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();

        let mut http_builder = Client::builder(TokioExecutor::new());
        http_builder.pool_idle_timeout(std::time::Duration::from_secs(20));
        let http_client = http_builder.build(connector);

        let auth_header = format!("Bearer {token}")
            .parse()
            .map_err(|e| Error::GitHub(format!("invalid token format: {e}")))?;

        let client = OctocrabBuilder::new_empty()
            .with_service(http_client)
            .with_layer(&BaseUriLayer::new(Uri::from_static(
                "https://api.github.com",
            )))
            .with_layer(&ExtraHeadersLayer::new(Arc::new(vec![
                (USER_AGENT, "octocrab".parse().unwrap()),
                (AUTHORIZATION, auth_header),
            ])))
            .with_auth(AuthState::None)
            .build()
            .unwrap();

        Ok(Self { client })
    }
}

/// Split "owner/name" into (owner, name).
fn parse_repo(repo: &str) -> Result<(&str, &str)> {
    repo.split_once('/').ok_or_else(|| {
        Error::GitHub(format!(
            "invalid repo format '{repo}', expected 'owner/name'"
        ))
    })
}

fn issue_state_str(state: octocrab::models::IssueState) -> String {
    match state {
        octocrab::models::IssueState::Open => "open".to_string(),
        octocrab::models::IssueState::Closed => "closed".to_string(),
        _ => "unknown".to_string(),
    }
}

#[async_trait]
impl GitHubClient for OctocrabClient {
    async fn fetch_issue(&self, repo: &str, number: u64) -> Result<IssueCandidate> {
        let (owner, name) = parse_repo(repo)?;
        let issue = self
            .client
            .issues(owner, name)
            .get(number)
            .await
            .map_err(|e| Error::GitHub(format!("fetch issue #{number}: {e}")))?;

        let comments = self
            .client
            .issues(owner, name)
            .list_comments(number)
            .per_page(100)
            .send()
            .await
            .map(|page| {
                page.items
                    .into_iter()
                    .map(|c| c.body.unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();

        Ok(IssueCandidate {
            number: issue.number,
            repo: repo.to_string(),
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            labels: issue.labels.into_iter().map(|l| l.name).collect(),
            state: issue_state_str(issue.state),
            created_at: issue.created_at.to_rfc3339(),
            updated_at: issue.updated_at.to_rfc3339(),
            is_assigned: !issue.assignees.is_empty(),
            html_url: issue.html_url.to_string(),
            author: issue.user.login.clone(),
            comments,
        })
    }

    async fn fetch_issue_state(&self, repo: &str, number: u64) -> Result<String> {
        let (owner, name) = parse_repo(repo)?;
        let issue = self
            .client
            .issues(owner, name)
            .get(number)
            .await
            .map_err(|e| Error::GitHub(format!("fetch issue state #{number}: {e}")))?;
        Ok(issue_state_str(issue.state))
    }

    async fn fetch_eligible_issues(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<IssueCandidate>> {
        let (owner, name) = parse_repo(repo)?;
        let label_vec: Vec<String> = labels.to_vec();
        let handler = self.client.issues(owner, name);
        let mut builder = handler
            .list()
            .state(octocrab::params::State::Open)
            .per_page(limit.min(100) as u8);

        if !label_vec.is_empty() {
            builder = builder.labels(&label_vec);
        }

        let page = builder
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("list issues: {e}")))?;

        Ok(page
            .items
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| IssueCandidate {
                number: i.number,
                repo: repo.to_string(),
                title: i.title,
                body: i.body.unwrap_or_default(),
                labels: i.labels.into_iter().map(|l| l.name).collect(),
                state: issue_state_str(i.state),
                created_at: i.created_at.to_rfc3339(),
                updated_at: i.updated_at.to_rfc3339(),
                is_assigned: !i.assignees.is_empty(),
                html_url: i.html_url.to_string(),
                author: i.user.login.clone(),
                comments: vec![],
            })
            .collect())
    }

    async fn fetch_issues_with_label(
        &self,
        repo: &str,
        label: &str,
    ) -> Result<Vec<IssueCandidate>> {
        self.fetch_eligible_issues(repo, &[label.to_string()], 100)
            .await
    }

    async fn add_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        let (owner, name) = parse_repo(repo)?;
        self.client
            .issues(owner, name)
            .add_labels(number, &[label.to_string()])
            .await
            .map_err(|e| Error::GitHub(format!("add label '{label}' to #{number}: {e}")))?;
        Ok(())
    }

    async fn remove_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        let (owner, name) = parse_repo(repo)?;
        let _ = self
            .client
            .issues(owner, name)
            .remove_label(number, label)
            .await;
        // Non-fatal — label may not exist.
        Ok(())
    }

    async fn create_label(
        &self,
        repo: &str,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<()> {
        let (owner, repo_name) = parse_repo(repo)?;
        let result = self
            .client
            .issues(owner, repo_name)
            .create_label(name, color, description)
            .await;
        if let Err(e) = result {
            // If it already exists, that's fine (idempotent).
            if !e.to_string().contains("already_exists") {
                return Err(Error::GitHub(format!("create label '{name}': {e}")));
            }
        }
        Ok(())
    }

    async fn comment_on_issue(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        let (owner, name) = parse_repo(repo)?;
        self.client
            .issues(owner, name)
            .create_comment(number, body)
            .await
            .map_err(|e| Error::GitHub(format!("comment on issue #{number}: {e}")))?;
        Ok(())
    }

    async fn fetch_pr(&self, repo: &str, number: u64) -> Result<PrCandidate> {
        let (owner, name) = parse_repo(repo)?;
        let pr = self
            .client
            .pulls(owner, name)
            .get(number)
            .await
            .map_err(|e| Error::GitHub(format!("fetch PR #{number}: {e}")))?;

        let checks_passing = fetch_checks_passing(&self.client, owner, name, &pr.head.sha).await;

        Ok(PrCandidate {
            number: pr.number,
            repo: repo.to_string(),
            title: pr.title.unwrap_or_default(),
            body: pr.body.unwrap_or_default(),
            labels: pr
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(|l| l.name)
                .collect(),
            state: pr
                .state
                .map(|s| match s {
                    octocrab::models::IssueState::Open => "open",
                    octocrab::models::IssueState::Closed => "closed",
                    _ => "unknown",
                })
                .unwrap_or("open")
                .to_string(),
            html_url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
            head_branch: pr.head.ref_field,
            base_branch: pr.base.ref_field,
            is_draft: pr.draft.unwrap_or(false),
            mergeable: pr.mergeable.map(|m| {
                if m {
                    "MERGEABLE".to_string()
                } else {
                    "CONFLICTING".to_string()
                }
            }),
            review_decision: None,
            checks_passing,
        })
    }

    async fn fetch_eligible_prs(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<PrCandidate>> {
        let (owner, name) = parse_repo(repo)?;
        let page = self
            .client
            .pulls(owner, name)
            .list()
            .state(octocrab::params::State::Open)
            .per_page(limit.min(100) as u8)
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("list PRs: {e}")))?;

        let label_set: std::collections::HashSet<&str> =
            labels.iter().map(|s| s.as_str()).collect();

        Ok(page
            .items
            .into_iter()
            .filter(|pr| {
                if label_set.is_empty() {
                    return true;
                }
                pr.labels
                    .as_ref()
                    .is_some_and(|pl| pl.iter().any(|l| label_set.contains(l.name.as_str())))
            })
            .map(|pr| PrCandidate {
                number: pr.number,
                repo: repo.to_string(),
                title: pr.title.unwrap_or_default(),
                body: String::new(),
                labels: pr
                    .labels
                    .unwrap_or_default()
                    .into_iter()
                    .map(|l| l.name)
                    .collect(),
                state: "open".to_string(),
                html_url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
                head_branch: pr.head.ref_field,
                base_branch: pr.base.ref_field,
                is_draft: pr.draft.unwrap_or(false),
                mergeable: None,
                review_decision: None,
                checks_passing: None,
            })
            .collect())
    }

    async fn fetch_prs_with_label(&self, repo: &str, label: &str) -> Result<Vec<PrCandidate>> {
        self.fetch_eligible_prs(repo, &[label.to_string()], 100)
            .await
    }

    async fn fetch_all_open_prs(&self, repo: &str, limit: usize) -> Result<Vec<PrCandidate>> {
        // The list endpoint doesn't return mergeable/checks — fetch the list
        // then enrich each PR with individual fetches so condition routes work.
        let (owner, name) = parse_repo(repo)?;
        let page = self
            .client
            .pulls(owner, name)
            .list()
            .state(octocrab::params::State::Open)
            .per_page(limit.min(100) as u8)
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("list PRs: {e}")))?;

        let mut results = Vec::with_capacity(page.items.len());
        for pr in page.items {
            let number = pr.number;
            match self.fetch_pr(repo, number).await {
                Ok(enriched) => results.push(enriched),
                Err(e) => {
                    tracing::warn!(pr = number, error = %e, "failed to enrich PR, using partial data");
                    results.push(PrCandidate {
                        number,
                        repo: repo.to_string(),
                        title: pr.title.unwrap_or_default(),
                        body: String::new(),
                        labels: pr
                            .labels
                            .unwrap_or_default()
                            .into_iter()
                            .map(|l| l.name)
                            .collect(),
                        state: "open".to_string(),
                        html_url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
                        head_branch: pr.head.ref_field,
                        base_branch: pr.base.ref_field,
                        is_draft: pr.draft.unwrap_or(false),
                        mergeable: None,
                        review_decision: None,
                        checks_passing: None,
                    });
                }
            }
        }
        Ok(results)
    }

    async fn fetch_pr_by_branch(&self, repo: &str, branch: &str) -> Result<Option<PrCandidate>> {
        let (owner, name) = parse_repo(repo)?;
        let page = self
            .client
            .pulls(owner, name)
            .list()
            .state(octocrab::params::State::Open)
            .head(format!("{owner}:{branch}"))
            .per_page(1)
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("list PRs by branch: {e}")))?;

        Ok(page.items.into_iter().next().map(|pr| PrCandidate {
            number: pr.number,
            repo: repo.to_string(),
            title: pr.title.unwrap_or_default(),
            body: pr.body.unwrap_or_default(),
            labels: pr
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(|l| l.name)
                .collect(),
            state: "open".to_string(),
            html_url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
            head_branch: pr.head.ref_field,
            base_branch: pr.base.ref_field,
            is_draft: pr.draft.unwrap_or(false),
            mergeable: None,
            review_decision: None,
            checks_passing: None,
        }))
    }

    async fn create_pull_request(
        &self,
        repo: &str,
        branch: &str,
        title: &str,
        body: &str,
        _work_dir: &Path,
        draft: bool,
    ) -> Result<PullRequest> {
        let (owner, name) = parse_repo(repo)?;
        let pr = self
            .client
            .pulls(owner, name)
            .create(title, branch, "main")
            .body(body)
            .draft(draft)
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("create PR: {e}")))?;

        Ok(PullRequest {
            number: pr.number,
            head_branch: branch.to_string(),
            state: "open".to_string(),
            html_url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
        })
    }

    async fn mark_pr_ready_for_review(&self, repo: &str, number: u64) -> Result<()> {
        let (owner, name) = parse_repo(repo)?;
        // Fetch the PR to get its node_id for the GraphQL mutation.
        let pr = self
            .client
            .pulls(owner, name)
            .get(number)
            .await
            .map_err(|e| Error::GitHub(format!("fetch PR #{number} for node_id: {e}")))?;
        let node_id = &pr.node_id;
        let body = serde_json::json!({
            "query": "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { number } } }",
            "variables": {"id": node_id}
        });
        let _: serde_json::Value = self
            .client
            .graphql(&body)
            .await
            .map_err(|e| Error::GitHub(format!("mark PR #{number} ready for review: {e}")))?;
        Ok(())
    }

    async fn update_pr_body(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        let (owner, name) = parse_repo(repo)?;
        self.client
            .pulls(owner, name)
            .update(number)
            .body(body)
            .send()
            .await
            .map_err(|e| Error::GitHub(format!("update PR body #{number}: {e}")))?;
        Ok(())
    }

    async fn add_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        self.add_label(repo, number, label).await
    }

    async fn remove_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()> {
        self.remove_label(repo, number, label).await
    }

    async fn comment_on_pr(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        self.comment_on_issue(repo, number, body).await
    }

    async fn fetch_authenticated_user(&self) -> Result<String> {
        let user = self
            .client
            .current()
            .user()
            .await
            .map_err(|e| Error::GitHub(format!("fetch authenticated user: {e}")))?;
        Ok(user.login)
    }
}

/// Fetch check runs for a commit and determine if all are passing.
async fn fetch_checks_passing(
    client: &Octocrab,
    owner: &str,
    name: &str,
    sha: &str,
) -> Option<bool> {
    let result = client
        .checks(owner, name)
        .list_check_runs_for_git_ref(octocrab::params::repos::Commitish(sha.to_string()))
        .send()
        .await;

    match result {
        Ok(runs) => {
            if runs.check_runs.is_empty() {
                return None;
            }
            let mut all_concluded = true;
            for run in &runs.check_runs {
                match run.conclusion.as_deref() {
                    Some("failure")
                    | Some("timed_out")
                    | Some("cancelled")
                    | Some("action_required") => return Some(false),
                    Some("success") | Some("skipped") | Some("neutral") => {}
                    _ => all_concluded = false,
                }
            }
            if all_concluded { Some(true) } else { None }
        }
        Err(_) => None,
    }
}
