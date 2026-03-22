//! Git client backed by the gix crate (pure Rust).
//!
//! Falls back to git CLI for operations not yet supported by gix
//! (worktree management, push, rebase).

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::GitClient;
use crate::error::{Error, Result};

/// Git client using the gix crate for local operations and
/// falling back to git CLI for network/worktree operations.
#[derive(Debug, Clone, Default)]
pub struct GixClient;

impl GixClient {
    pub fn new() -> Self {
        Self
    }
}

/// Open a gix repository at the given path.
fn open_repo(dir: &Path) -> Result<gix::Repository> {
    gix::open(dir).map_err(|e| Error::Git(format!("failed to open repo at {}: {e}", dir.display())))
}

/// Run a git CLI command (for operations gix doesn't support yet).
async fn git_cli(args: &[&str], dir: &Path) -> Result<std::process::Output> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    Ok(output)
}

#[async_trait]
impl GitClient for GixClient {
    async fn fetch(&self, repo_dir: &Path) -> Result<()> {
        // gix fetch support is complex — use CLI for now.
        let output = git_cli(&["fetch", "origin"], repo_dir).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("git fetch failed: {stderr}")));
        }
        Ok(())
    }

    async fn worktree_add(
        &self,
        repo_dir: &Path,
        branch: &str,
        worktree_base: &str,
    ) -> Result<PathBuf> {
        // gix doesn't support worktree management — use CLI.
        let dir_name = branch.replace('/', "-");
        let worktree_dir = repo_dir.join(worktree_base).join(&dir_name);

        let _ = git_cli(&["fetch", "origin"], repo_dir).await;

        let remote_ref = format!("origin/{branch}");
        let remote_exists = git_cli(&["rev-parse", "--verify", &remote_ref], repo_dir)
            .await
            .is_ok_and(|o| o.status.success());
        let local_exists = git_cli(&["rev-parse", "--verify", branch], repo_dir)
            .await
            .is_ok_and(|o| o.status.success());

        let dir_str = worktree_dir.to_string_lossy().to_string();
        let output = if remote_exists || local_exists {
            git_cli(&["worktree", "add", &dir_str, branch], repo_dir).await?
        } else {
            git_cli(
                &["worktree", "add", "-b", branch, &dir_str, "origin/main"],
                repo_dir,
            )
            .await?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("failed to create worktree: {stderr}")));
        }

        Ok(worktree_dir)
    }

    async fn worktree_remove(
        &self,
        repo_dir: &Path,
        worktree_dir: &Path,
        force: bool,
    ) -> Result<()> {
        // gix doesn't support worktree management — use CLI.
        let dir_str = worktree_dir.to_string_lossy().to_string();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&dir_str);

        let output = git_cli(&args, repo_dir).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("failed to remove worktree: {stderr}")));
        }
        Ok(())
    }

    async fn remote_url(&self, repo_dir: &Path) -> Result<String> {
        let repo = open_repo(repo_dir)?;
        let remote = repo
            .find_remote("origin")
            .map_err(|e| Error::Git(format!("remote 'origin' not found: {e}")))?;
        let url = remote
            .url(gix::remote::Direction::Fetch)
            .ok_or_else(|| Error::Git("origin has no fetch URL".into()))?;
        Ok(url.to_bstring().to_string())
    }

    async fn ref_exists(&self, repo_dir: &Path, ref_name: &str) -> Result<bool> {
        let repo = open_repo(repo_dir)?;
        Ok(repo.find_reference(ref_name).is_ok())
    }

    async fn has_changes(&self, work_dir: &Path) -> Result<bool> {
        // gix status is complex — use CLI for reliability.
        let output = git_cli(&["status", "--porcelain"], work_dir).await?;
        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    async fn stage_tracked(&self, work_dir: &Path) -> Result<()> {
        // gix index manipulation is complex — use CLI.
        let _ = git_cli(&["add", "-u"], work_dir).await?;
        Ok(())
    }

    async fn commit(&self, work_dir: &Path, message: &str) -> Result<()> {
        // gix commit creation requires tree building — use CLI.
        let _ = git_cli(&["commit", "-m", message], work_dir).await?;
        Ok(())
    }

    async fn rebase(&self, work_dir: &Path, onto: &str) -> Result<bool> {
        // gix doesn't support rebase — use CLI.
        let output = git_cli(&["rebase", onto], work_dir).await?;
        Ok(output.status.success())
    }

    async fn rebase_abort(&self, work_dir: &Path) -> Result<()> {
        let _ = git_cli(&["rebase", "--abort"], work_dir).await?;
        Ok(())
    }

    async fn diff_stat(&self, work_dir: &Path, base: &str) -> Result<String> {
        // gix diff is complex — use CLI.
        let output = git_cli(&["diff", "--stat", base], work_dir).await?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn push(&self, work_dir: &Path, branch: &str) -> Result<()> {
        // gix push requires transport setup — use CLI.
        let output = git_cli(&["push", "-u", "origin", branch], work_dir).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("git push failed: {stderr}")));
        }
        Ok(())
    }

    async fn push_force(&self, work_dir: &Path, branch: &str) -> Result<()> {
        let output = git_cli(
            &["push", "--force-with-lease", "-u", "origin", branch],
            work_dir,
        )
        .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("git push failed: {stderr}")));
        }
        Ok(())
    }

    async fn version(&self) -> Result<String> {
        Ok(format!("gix {}", env!("CARGO_PKG_VERSION")))
    }
}
