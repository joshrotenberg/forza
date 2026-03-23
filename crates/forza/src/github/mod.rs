//! GitHub platform adapter — issues, PRs, comments, labels.
//!
//! Defines the `GitHubClient` trait for platform abstraction.
//! Current implementations:
//! - `GhCliClient` (default) — uses the `gh` CLI
//! - `OctocrabClient` (planned) — uses the octocrab crate

mod gh_cli;
mod octocrab_client;

pub use gh_cli::GhCliClient;
pub use octocrab_client::OctocrabClient;

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Trait abstracting all GitHub API operations.
///
/// Implementations handle auth, pagination, and rate limiting.
/// Git operations (push, worktree) are NOT part of this trait —
/// those remain as free functions since they're git, not GitHub API.
#[async_trait]
pub trait GitHubClient: Send + Sync {
    // ── Issues ──────────────────────────────────────────────────────
    async fn fetch_issue(&self, repo: &str, number: u64) -> Result<IssueCandidate>;
    async fn fetch_issue_state(&self, repo: &str, number: u64) -> Result<String>;
    async fn fetch_eligible_issues(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<IssueCandidate>>;
    async fn fetch_issues_with_label(&self, repo: &str, label: &str)
    -> Result<Vec<IssueCandidate>>;
    async fn add_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;
    async fn remove_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;
    async fn create_label(
        &self,
        repo: &str,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<()>;
    async fn comment_on_issue(&self, repo: &str, number: u64, body: &str) -> Result<()>;

    // ── Pull Requests ───────────────────────────────────────────────
    async fn fetch_pr(&self, repo: &str, number: u64) -> Result<PrCandidate>;
    async fn fetch_eligible_prs(
        &self,
        repo: &str,
        labels: &[String],
        limit: usize,
    ) -> Result<Vec<PrCandidate>>;
    async fn fetch_prs_with_label(&self, repo: &str, label: &str) -> Result<Vec<PrCandidate>>;
    async fn fetch_all_open_prs(&self, repo: &str, limit: usize) -> Result<Vec<PrCandidate>>;
    async fn fetch_pr_by_branch(&self, repo: &str, branch: &str) -> Result<Option<PrCandidate>>;
    async fn create_pull_request(
        &self,
        repo: &str,
        branch: &str,
        title: &str,
        body: &str,
        work_dir: &Path,
        draft: bool,
    ) -> Result<PullRequest>;
    async fn mark_pr_ready_for_review(&self, repo: &str, number: u64) -> Result<()>;
    async fn update_pr_body(&self, repo: &str, number: u64, body: &str) -> Result<()>;
    async fn add_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;
    async fn remove_pr_label(&self, repo: &str, number: u64, label: &str) -> Result<()>;
    async fn comment_on_pr(&self, repo: &str, number: u64, body: &str) -> Result<()>;

    // ── Auth ────────────────────────────────────────────────────────
    async fn fetch_authenticated_user(&self) -> Result<String>;
}

/// Returns the default GitHub client (GhCliClient).
///
/// This will be replaced with configurable client selection when
/// OctocrabClient is implemented.
pub fn default_client() -> &'static GhCliClient {
    static CLIENT: GhCliClient = GhCliClient;
    &CLIENT
}

/// A normalized issue representation, independent of GitHub API shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCandidate {
    pub number: u64,
    pub repo: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_assigned: bool,
    pub html_url: String,
    /// Login of the issue author.
    #[serde(default)]
    pub author: String,
    /// Issue comments (discussion, design decisions).
    #[serde(default)]
    pub comments: Vec<String>,
}

/// Minimal PR representation for tracking automation-owned PRs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub head_branch: String,
    pub state: String,
    pub html_url: String,
}

/// A normalized PR representation for PR workflows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCandidate {
    pub number: u64,
    pub repo: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub state: String,
    pub html_url: String,
    pub head_branch: String,
    pub base_branch: String,
    pub is_draft: bool,
    pub mergeable: Option<String>,
    pub review_decision: Option<String>,
    /// Whether all required CI checks are passing. `None` if unknown.
    #[serde(default)]
    pub checks_passing: Option<bool>,
}

// ── Raw gh CLI JSON shapes (private) ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    body: Option<String>,
    labels: Vec<GhLabel>,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    assignees: Vec<serde_json::Value>,
    url: String,
    author: GhAuthor,
    #[serde(default)]
    comments: Vec<GhComment>,
}

#[derive(Debug, Deserialize)]
struct GhAuthor {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GhComment {
    body: String,
}

#[derive(Debug, Deserialize)]
struct GhStatusCheck {
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhPrFull {
    number: u64,
    title: String,
    body: Option<String>,
    labels: Vec<GhLabel>,
    state: String,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<GhStatusCheck>,
}

// Raw shape for `gh pr list` in reactive workflows (minimal fields).
#[derive(Debug, Deserialize)]
struct GhPrRaw {
    number: u64,
    title: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    state: String,
    labels: Vec<GhLabel>,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
    url: String,
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<GhStatusCheck>,
}

/// Compute whether all checks are passing from a status check rollup.
fn checks_passing(rollup: &[GhStatusCheck]) -> Option<bool> {
    if rollup.is_empty() {
        return None;
    }
    // If any check has conclusion FAILURE, checks are failing.
    // If all concluded checks are SUCCESS/SKIPPED/NEUTRAL, checks are passing.
    // If some checks have no conclusion yet (in progress), result is None.
    let mut all_concluded = true;
    for check in rollup {
        match check.conclusion.as_deref() {
            Some("FAILURE") | Some("TIMED_OUT") | Some("CANCELLED") | Some("ACTION_REQUIRED") => {
                return Some(false);
            }
            Some("SUCCESS") | Some("SKIPPED") | Some("NEUTRAL") => {}
            None => all_concluded = false,
            Some(_) => return Some(false),
        }
    }
    if all_concluded { Some(true) } else { None }
}

// ── Public API ───────────────────────────────────────────────────────

/// Fetch an issue from GitHub.
pub async fn fetch_issue(repo: &str, number: u64) -> Result<IssueCandidate> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "view",
            "--repo",
            repo,
            &number.to_string(),
            "--json",
            "number,title,body,labels,state,createdAt,updatedAt,assignees,url,author,comments",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lower = stderr.to_lowercase();
        if lower.contains("not found") || lower.contains("could not resolve") {
            return Err(Error::GitHub(format!(
                "issue #{number} not found in {repo}"
            )));
        }
        return Err(Error::GitHub(format!(
            "fetch issue #{number} in {repo}: {stderr}"
        )));
    }

    let raw: GhIssue = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(IssueCandidate {
        number: raw.number,
        repo: repo.to_string(),
        title: raw.title,
        body: raw.body.unwrap_or_default(),
        labels: raw.labels.into_iter().map(|l| l.name).collect(),
        state: raw.state,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        is_assigned: !raw.assignees.is_empty(),
        html_url: raw.url,
        author: raw.author.login,
        comments: raw.comments.into_iter().map(|c| c.body).collect(),
    })
}

/// Fetch the state of a single issue (e.g. `"OPEN"` or `"CLOSED"`).
///
/// Lightweight — only fetches the `state` field, not the full issue body.
pub async fn fetch_issue_state(repo: &str, number: u64) -> Result<String> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "view",
            "--repo",
            repo,
            &number.to_string(),
            "--json",
            "state",
            "--jq",
            ".state",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh issue view failed: {stderr}")));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fetch multiple open issues matching eligible labels.
pub async fn fetch_eligible_issues(
    repo: &str,
    labels: &[String],
    limit: usize,
) -> Result<Vec<IssueCandidate>> {
    let mut args = vec![
        "issue".to_string(),
        "list".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--state".to_string(),
        "open".to_string(),
        "--json".to_string(),
        "number,title,body,labels,state,createdAt,updatedAt,assignees,url,author,comments"
            .to_string(),
        "--limit".to_string(),
        limit.to_string(),
    ];

    for label in labels {
        args.push("--label".to_string());
        args.push(label.clone());
    }

    let output = tokio::process::Command::new("gh")
        .args(&args)
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh issue list failed: {stderr}")));
    }

    let raw: Vec<GhIssue> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| IssueCandidate {
            number: r.number,
            repo: repo.to_string(),
            title: r.title,
            body: r.body.unwrap_or_default(),
            labels: r.labels.into_iter().map(|l| l.name).collect(),
            state: r.state,
            created_at: r.created_at,
            updated_at: r.updated_at,
            is_assigned: !r.assignees.is_empty(),
            html_url: r.url,
            author: r.author.login,
            comments: r.comments.into_iter().map(|c| c.body).collect(),
        })
        .collect())
}

/// Fetch all open issues that have a specific label.
pub async fn fetch_issues_with_label(repo: &str, label: &str) -> Result<Vec<IssueCandidate>> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--label",
            label,
            "--json",
            "number,title,body,labels,state,createdAt,updatedAt,assignees,url,author,comments",
            "--limit",
            "100",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!(
            "gh issue list (label={label}) failed: {stderr}"
        )));
    }

    let raw: Vec<GhIssue> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| IssueCandidate {
            number: r.number,
            repo: repo.to_string(),
            title: r.title,
            body: r.body.unwrap_or_default(),
            labels: r.labels.into_iter().map(|l| l.name).collect(),
            state: r.state,
            created_at: r.created_at,
            updated_at: r.updated_at,
            is_assigned: !r.assignees.is_empty(),
            html_url: r.url,
            author: r.author.login,
            comments: r.comments.into_iter().map(|c| c.body).collect(),
        })
        .collect())
}

/// Fetch the login of the currently authenticated GitHub user.
pub async fn fetch_authenticated_user() -> Result<String> {
    let output = tokio::process::Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh api user: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh api user failed: {stderr}")));
    }

    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if login.is_empty() {
        return Err(Error::GitHub("gh api user returned empty login".into()));
    }
    Ok(login)
}

/// Push a branch from a worktree to the remote.
pub async fn push_branch(
    work_dir: &Path,
    branch: &str,
    git: &dyn crate::git::GitClient,
) -> Result<()> {
    git.push(work_dir, branch).await
}

/// Push a branch with --force-with-lease (handles stale remote branches).
pub async fn push_branch_force(
    work_dir: &Path,
    branch: &str,
    git: &dyn crate::git::GitClient,
) -> Result<()> {
    git.push_force(work_dir, branch).await
}

/// Create a pull request via gh CLI.
pub async fn create_pull_request(
    repo: &str,
    branch: &str,
    title: &str,
    body: &str,
    work_dir: &Path,
    draft: bool,
) -> Result<PullRequest> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--head".to_string(),
        branch.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];
    if draft {
        args.push("--draft".to_string());
    }
    let output = tokio::process::Command::new("gh")
        .args(&args)
        .current_dir(work_dir)
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr create failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr create failed: {stderr}")));
    }

    // gh pr create outputs the PR URL on stdout. Fetch details.
    let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Extract PR number from URL.
    let pr_number = pr_url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Ok(PullRequest {
        number: pr_number,
        head_branch: branch.to_string(),
        state: "open".to_string(),
        html_url: pr_url,
    })
}

/// Add a label to an issue.
pub async fn add_label(repo: &str, number: u64, label: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "edit",
            "--repo",
            repo,
            &number.to_string(),
            "--add-label",
            label,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh issue edit failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("add label failed: {stderr}")));
    }
    Ok(())
}

/// Remove a label from an issue.
pub async fn remove_label(repo: &str, number: u64, label: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "edit",
            "--repo",
            repo,
            &number.to_string(),
            "--remove-label",
            label,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh issue edit failed: {e}")))?;

    if !output.status.success() {
        tracing::debug!(label = label, "remove label failed (non-fatal)");
    }
    Ok(())
}

/// Create a label in a repository (idempotent — uses --force to update if it exists).
pub async fn create_label(repo: &str, name: &str, color: &str, description: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "label",
            "create",
            "--repo",
            repo,
            name,
            "--color",
            color,
            "--description",
            description,
            "--force",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh label create failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh label create failed: {stderr}")));
    }

    Ok(())
}

/// Fetch open PRs matching a set of labels for reactive maintenance workflows.
pub async fn fetch_eligible_prs(
    repo: &str,
    labels: &[String],
    limit: usize,
) -> Result<Vec<PrCandidate>> {
    let mut args = vec![
        "pr".to_string(),
        "list".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--state".to_string(),
        "open".to_string(),
        "--json".to_string(),
        "number,title,headRefName,state,labels,mergeable,reviewDecision,url".to_string(),
        "--limit".to_string(),
        limit.to_string(),
    ];

    for label in labels {
        args.push("--label".to_string());
        args.push(label.clone());
    }

    let output = tokio::process::Command::new("gh")
        .args(&args)
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr list failed: {stderr}")));
    }

    let raw: Vec<GhPrRaw> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| {
            let cp = checks_passing(&r.status_check_rollup);
            PrCandidate {
                number: r.number,
                repo: repo.to_string(),
                title: r.title,
                body: String::new(),
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                state: r.state,
                html_url: r.url,
                head_branch: r.head_ref_name,
                base_branch: String::new(),
                is_draft: false,
                mergeable: r.mergeable,
                review_decision: r.review_decision,
                checks_passing: cp,
            }
        })
        .collect())
}

/// Fetch a PR from GitHub.
pub async fn fetch_pr(repo: &str, number: u64) -> Result<PrCandidate> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "view",
            "--repo",
            repo,
            &number.to_string(),
            "--json",
            "number,title,body,labels,state,url,headRefName,baseRefName,isDraft,mergeable,reviewDecision,statusCheckRollup",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lower = stderr.to_lowercase();
        if lower.contains("not found") || lower.contains("could not resolve") {
            return Err(Error::GitHub(format!("PR #{number} not found in {repo}")));
        }
        return Err(Error::GitHub(format!(
            "fetch PR #{number} in {repo}: {stderr}"
        )));
    }

    let raw: GhPrFull = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    let cp = checks_passing(&raw.status_check_rollup);
    Ok(PrCandidate {
        number: raw.number,
        repo: repo.to_string(),
        title: raw.title,
        body: raw.body.unwrap_or_default(),
        labels: raw.labels.into_iter().map(|l| l.name).collect(),
        state: raw.state,
        html_url: raw.url,
        head_branch: raw.head_ref_name,
        base_branch: raw.base_ref_name,
        is_draft: raw.is_draft,
        mergeable: raw.mergeable,
        review_decision: raw.review_decision,
        checks_passing: cp,
    })
}

/// Fetch all open PRs that have a specific label.
pub async fn fetch_prs_with_label(repo: &str, label: &str) -> Result<Vec<PrCandidate>> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--label",
            label,
            "--json",
            "number,title,body,labels,state,url,headRefName,baseRefName,isDraft,mergeable,reviewDecision,statusCheckRollup",
            "--limit",
            "100",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!(
            "gh pr list (label={label}) failed: {stderr}"
        )));
    }

    let raw: Vec<GhPrFull> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| {
            let cp = checks_passing(&r.status_check_rollup);
            PrCandidate {
                number: r.number,
                repo: repo.to_string(),
                title: r.title,
                body: r.body.unwrap_or_default(),
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                state: r.state,
                html_url: r.url,
                head_branch: r.head_ref_name,
                base_branch: r.base_ref_name,
                is_draft: r.is_draft,
                mergeable: r.mergeable,
                review_decision: r.review_decision,
                checks_passing: cp,
            }
        })
        .collect())
}

/// Fetch all open PRs in a repo (no label filter).
pub async fn fetch_all_open_prs(repo: &str, limit: usize) -> Result<Vec<PrCandidate>> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--json",
            "number,title,body,labels,state,url,headRefName,baseRefName,isDraft,mergeable,reviewDecision,statusCheckRollup",
            "--limit",
            &limit.to_string(),
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr list failed: {stderr}")));
    }

    let raw: Vec<GhPrFull> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| {
            let cp = checks_passing(&r.status_check_rollup);
            PrCandidate {
                number: r.number,
                repo: repo.to_string(),
                title: r.title,
                body: r.body.unwrap_or_default(),
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                state: r.state,
                html_url: r.url,
                head_branch: r.head_ref_name,
                base_branch: r.base_ref_name,
                is_draft: r.is_draft,
                mergeable: r.mergeable,
                review_decision: r.review_decision,
                checks_passing: cp,
            }
        })
        .collect())
}

/// Post a comment on a PR.
pub async fn comment_on_pr(repo: &str, number: u64, body: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "comment",
            "--repo",
            repo,
            &number.to_string(),
            "--body",
            body,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr comment failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr comment failed: {stderr}")));
    }

    Ok(())
}

/// Fetch an open PR by its head branch name. Returns None if no PR exists.
pub async fn fetch_pr_by_branch(repo: &str, branch: &str) -> Result<Option<PrCandidate>> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--head",
            branch,
            "--state",
            "open",
            "--json",
            "number,title,body,labels,state,url,headRefName,baseRefName,isDraft,mergeable,reviewDecision,statusCheckRollup",
            "--limit",
            "1",
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr list --head failed: {stderr}")));
    }

    let raw: Vec<GhPrFull> = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error::GitHub(format!("failed to parse gh output: {e}")))?;

    Ok(raw.into_iter().next().map(|r| {
        let cp = checks_passing(&r.status_check_rollup);
        PrCandidate {
            number: r.number,
            repo: repo.to_string(),
            title: r.title,
            body: r.body.unwrap_or_default(),
            labels: r.labels.into_iter().map(|l| l.name).collect(),
            state: r.state,
            html_url: r.url,
            head_branch: r.head_ref_name,
            base_branch: r.base_ref_name,
            is_draft: r.is_draft,
            mergeable: r.mergeable,
            review_decision: r.review_decision,
            checks_passing: cp,
        }
    }))
}

/// Add a label to a PR.
pub async fn add_pr_label(repo: &str, number: u64, label: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "edit",
            "--repo",
            repo,
            &number.to_string(),
            "--add-label",
            label,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr edit failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("add pr label failed: {stderr}")));
    }
    Ok(())
}

/// Remove a label from a PR.
pub async fn remove_pr_label(repo: &str, number: u64, label: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "edit",
            "--repo",
            repo,
            &number.to_string(),
            "--remove-label",
            label,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr edit failed: {e}")))?;

    if !output.status.success() {
        tracing::debug!(label = label, "remove pr label failed (non-fatal)");
    }
    Ok(())
}

/// Mark a draft PR as ready for review via gh CLI.
pub async fn mark_pr_ready_for_review(repo: &str, number: u64) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args(["pr", "ready", &number.to_string(), "--repo", repo])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr ready failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr ready failed: {stderr}")));
    }
    Ok(())
}

/// Update the body of an existing PR via gh CLI.
pub async fn update_pr_body(repo: &str, number: u64, body: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "edit",
            &number.to_string(),
            "--repo",
            repo,
            "--body",
            body,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh pr edit failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh pr edit failed: {stderr}")));
    }
    Ok(())
}

/// Post a comment on an issue.
pub async fn comment_on_issue(repo: &str, number: u64, body: &str) -> Result<()> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "comment",
            "--repo",
            repo,
            &number.to_string(),
            "--body",
            body,
        ])
        .output()
        .await
        .map_err(|e| Error::GitHub(format!("gh issue comment failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitHub(format!("gh issue comment failed: {stderr}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_check(conclusion: Option<&str>) -> GhStatusCheck {
        GhStatusCheck {
            conclusion: conclusion.map(|s| s.to_string()),
        }
    }

    #[test]
    fn checks_passing_empty_rollup_returns_none() {
        assert_eq!(checks_passing(&[]), None);
    }

    #[test]
    fn checks_passing_all_success() {
        let rollup = vec![
            make_check(Some("SUCCESS")),
            make_check(Some("SKIPPED")),
            make_check(Some("NEUTRAL")),
        ];
        assert_eq!(checks_passing(&rollup), Some(true));
    }

    #[test]
    fn checks_passing_failure_returns_false() {
        let rollup = vec![make_check(Some("SUCCESS")), make_check(Some("FAILURE"))];
        assert_eq!(checks_passing(&rollup), Some(false));
    }

    #[test]
    fn checks_passing_timed_out_returns_false() {
        let rollup = vec![make_check(Some("TIMED_OUT"))];
        assert_eq!(checks_passing(&rollup), Some(false));
    }

    #[test]
    fn checks_passing_pending_conclusion_returns_none() {
        let rollup = vec![make_check(Some("SUCCESS")), make_check(None)];
        assert_eq!(checks_passing(&rollup), None);
    }

    #[test]
    fn checks_passing_unknown_conclusion_returns_false() {
        // Any concluded-but-unknown state (STARTUP_FAILURE, STALE, future values)
        // is treated as a failure rather than leaving the PR in a dead zone.
        let rollup = vec![make_check(Some("STARTUP_FAILURE"))];
        assert_eq!(checks_passing(&rollup), Some(false));

        let rollup = vec![make_check(Some("STALE"))];
        assert_eq!(checks_passing(&rollup), Some(false));
    }
}
