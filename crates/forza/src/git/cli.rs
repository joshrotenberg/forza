//! Git client backed by the `git` CLI.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::warn;

use super::GitClient;
use crate::error::{Error, Result};

/// Git client that shells out to the `git` CLI.
#[derive(Debug, Clone, Default)]
pub struct GitCliClient;

impl GitCliClient {
    pub fn new() -> Self {
        Self
    }
}

/// Run a git command and return the output.
async fn git(args: &[&str], dir: &Path) -> std::result::Result<std::process::Output, Error> {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))
}

/// Run a git command, check for success, return stdout.
async fn git_ok(args: &[&str], dir: &Path) -> Result<String> {
    let output = git(args, dir).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Git(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[async_trait]
impl GitClient for GitCliClient {
    async fn fetch(&self, repo_dir: &Path) -> Result<()> {
        let _ = git(&["fetch", "origin"], repo_dir).await?;
        Ok(())
    }

    async fn worktree_add(
        &self,
        repo_dir: &Path,
        branch: &str,
        worktree_base: &str,
    ) -> Result<PathBuf> {
        // Sanitize branch name for directory.
        let dir_name = branch.replace('/', "-");
        let worktree_dir = repo_dir.join(worktree_base).join(&dir_name);

        // Fetch to ensure we have latest refs.
        if let Err(e) = git(&["fetch", "origin"], repo_dir).await {
            warn!(error = %e, "fetch origin failed (non-fatal)");
        }

        // Check if branch already exists on remote or locally.
        let remote_ref = format!("origin/{branch}");
        let remote_exists = git(&["rev-parse", "--verify", &remote_ref], repo_dir)
            .await
            .is_ok_and(|o| o.status.success());

        let local_exists = git(&["rev-parse", "--verify", branch], repo_dir)
            .await
            .is_ok_and(|o| o.status.success());

        let output = if remote_exists || local_exists {
            // Branch exists — check it out in worktree.
            git(
                &["worktree", "add", &worktree_dir.to_string_lossy(), branch],
                repo_dir,
            )
            .await?
        } else {
            // New branch — create from the repo's default branch.
            let base = self.default_branch(repo_dir).await?;
            git(
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    &worktree_dir.to_string_lossy(),
                    &base,
                ],
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
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        let dir_str = worktree_dir.to_string_lossy();
        args.push(&dir_str);

        let output = git(&args, repo_dir).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("failed to remove worktree: {stderr}")));
        }
        Ok(())
    }

    async fn remote_url(&self, repo_dir: &Path) -> Result<String> {
        let output = git_ok(&["remote", "get-url", "origin"], repo_dir).await?;
        Ok(output.trim().to_string())
    }

    async fn ref_exists(&self, repo_dir: &Path, ref_name: &str) -> Result<bool> {
        let output = git(&["rev-parse", "--verify", ref_name], repo_dir).await?;
        Ok(output.status.success())
    }

    async fn has_changes(&self, work_dir: &Path) -> Result<bool> {
        let output = git_ok(&["status", "--porcelain"], work_dir).await?;
        Ok(!output.trim().is_empty())
    }

    async fn stage_tracked(&self, work_dir: &Path) -> Result<()> {
        let _ = git(&["add", "-u"], work_dir).await?;
        Ok(())
    }

    async fn stage_path(&self, work_dir: &Path, path: &str) -> Result<()> {
        let _ = git(&["add", path], work_dir).await?;
        Ok(())
    }

    async fn commit(&self, work_dir: &Path, message: &str) -> Result<()> {
        let _ = git(&["commit", "-m", message], work_dir).await?;
        Ok(())
    }

    async fn rebase(&self, work_dir: &Path, onto: &str) -> Result<bool> {
        let output = git(&["rebase", onto], work_dir).await?;
        Ok(output.status.success())
    }

    async fn rebase_abort(&self, work_dir: &Path) -> Result<()> {
        let _ = git(&["rebase", "--abort"], work_dir).await?;
        Ok(())
    }

    async fn diff_stat(&self, work_dir: &Path, base: &str) -> Result<String> {
        let output = git(&["diff", "--stat", base], work_dir).await?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn push(&self, work_dir: &Path, branch: &str) -> Result<()> {
        git_ok(&["push", "-u", "origin", branch], work_dir).await?;
        Ok(())
    }

    async fn push_force(&self, work_dir: &Path, branch: &str) -> Result<()> {
        git_ok(
            &["push", "--force-with-lease", "-u", "origin", branch],
            work_dir,
        )
        .await?;
        Ok(())
    }

    async fn create_branch_from(&self, repo_dir: &Path, branch: &str, base: &str) -> Result<()> {
        if let Err(e) = git(&["fetch", "origin"], repo_dir).await {
            warn!(error = %e, "fetch origin failed (non-fatal)");
        }
        let already_exists = git(&["rev-parse", "--verify", branch], repo_dir)
            .await
            .is_ok_and(|o| o.status.success());
        if already_exists {
            return Ok(());
        }
        git_ok(&["branch", branch, base], repo_dir).await?;
        Ok(())
    }

    async fn default_branch(&self, repo_dir: &Path) -> Result<String> {
        let output = git(&["symbolic-ref", "refs/remotes/origin/HEAD"], repo_dir).await?;
        if output.status.success() {
            let sym = String::from_utf8_lossy(&output.stdout);
            // "refs/remotes/origin/main\n" -> strip "refs/remotes/" prefix
            let branch = sym.trim().trim_start_matches("refs/remotes/");
            if !branch.is_empty() {
                return Ok(branch.to_string());
            }
        }
        Ok("origin/main".to_string())
    }

    async fn worktree_prune(&self, repo_dir: &Path) -> Result<()> {
        let output = git(&["worktree", "prune"], repo_dir).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Git(format!("git worktree prune failed: {stderr}")));
        }
        Ok(())
    }

    async fn version(&self) -> Result<String> {
        let output = tokio::process::Command::new("git")
            .args(["--version"])
            .output()
            .await
            .map_err(|e| Error::Git(format!("git not found: {e}")))?;
        if !output.status.success() {
            return Err(Error::Git("git --version failed".into()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
