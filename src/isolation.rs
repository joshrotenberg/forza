//! Work isolation — git worktrees and cleanup.

use std::path::{Path, PathBuf};

/// Create an isolated work directory for a run.
pub async fn create_worktree(
    repo_dir: &Path,
    branch: &str,
    base_dir: &str,
) -> crate::error::Result<PathBuf> {
    let slug: String = branch
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let worktree_dir = repo_dir.join(base_dir).join(&slug);

    tokio::fs::create_dir_all(repo_dir.join(base_dir)).await?;

    // Fetch latest from remote so we branch from up-to-date main.
    let _ = tokio::process::Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(repo_dir)
        .output()
        .await;

    // Determine the default branch base point.
    let base_ref = detect_default_branch(repo_dir).await;

    let output = tokio::process::Command::new("git")
        .args(["worktree", "add", "-b", branch])
        .arg(&worktree_dir)
        .arg(&base_ref)
        .current_dir(repo_dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Branch may already exist — try without -b.
        if stderr.contains("already exists") {
            let output = tokio::process::Command::new("git")
                .args(["worktree", "add"])
                .arg(&worktree_dir)
                .arg(branch)
                .current_dir(repo_dir)
                .output()
                .await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(crate::error::Error::Isolation(format!(
                    "failed to create worktree: {stderr}"
                )));
            }
        } else {
            return Err(crate::error::Error::Isolation(format!(
                "failed to create worktree: {stderr}"
            )));
        }
    }

    Ok(worktree_dir)
}

/// Remove a worktree.
pub async fn remove_worktree(
    repo_dir: &Path,
    worktree_dir: &Path,
    force: bool,
) -> crate::error::Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    let path_str = worktree_dir.to_string_lossy().to_string();
    args.push(&path_str);

    let output = tokio::process::Command::new("git")
        .args(&args)
        .current_dir(repo_dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::Error::Isolation(format!(
            "failed to remove worktree: {stderr}"
        )));
    }

    Ok(())
}

/// List all worktrees under `repo_dir/base_dir`.
pub fn list_worktrees(repo_dir: &Path, base_dir: &str) -> Vec<PathBuf> {
    let dir = repo_dir.join(base_dir);
    std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Detect the remote default branch (origin/main or origin/master).
async fn detect_default_branch(repo_dir: &Path) -> String {
    // Try origin/main first, fall back to origin/master, then HEAD.
    for candidate in &["origin/main", "origin/master"] {
        let output = tokio::process::Command::new("git")
            .args(["rev-parse", "--verify", candidate])
            .current_dir(repo_dir)
            .output()
            .await;
        if let Ok(o) = output
            && o.status.success()
        {
            return candidate.to_string();
        }
    }
    "HEAD".to_string()
}
