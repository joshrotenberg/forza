//! Work isolation — git worktrees and cleanup.

use std::path::{Path, PathBuf};

use crate::git::GitClient;

/// Sentinel file written inside a worktree to record the owning PID.
const OWNER_FILE: &str = ".forza-owner";

/// Write the current process PID to `{worktree_dir}/.forza-owner`.
///
/// Best-effort: logs a warning on failure but does not abort.
fn write_owner_file(worktree_dir: &Path) {
    let pid = std::process::id().to_string();
    let path = worktree_dir.join(OWNER_FILE);
    if let Err(e) = std::fs::write(&path, &pid) {
        tracing::warn!(path = %path.display(), error = %e, "failed to write owner file");
    }
}

/// Return `true` if the process that created `worktree_dir` is still alive.
///
/// Reads `.forza-owner`, parses the PID, then sends signal 0 to check
/// liveness.  Returns `false` if the file is missing, unparseable, or the
/// process is no longer running.
fn worktree_is_active(worktree_dir: &Path) -> bool {
    let path = worktree_dir.join(OWNER_FILE);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(pid_str) = contents.trim().parse::<u32>().map(|p| p.to_string()) else {
        return false;
    };
    std::process::Command::new("kill")
        .args(["-0", &pid_str])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Create an isolated work directory for a run.
pub async fn create_worktree(
    repo_dir: &Path,
    branch: &str,
    base_dir: &str,
    git: &dyn GitClient,
) -> crate::error::Result<PathBuf> {
    tokio::fs::create_dir_all(repo_dir.join(base_dir)).await?;

    // The trait's worktree_add handles fetch, branch detection, and checkout.
    let worktree_dir = git
        .worktree_add(repo_dir, branch, base_dir)
        .await
        .map_err(|e| crate::error::Error::Isolation(format!("failed to create worktree: {e}")))?;
    write_owner_file(&worktree_dir);
    Ok(worktree_dir)
}

/// Remove a worktree.
pub async fn remove_worktree(
    repo_dir: &Path,
    worktree_dir: &Path,
    force: bool,
    git: &dyn GitClient,
) -> crate::error::Result<()> {
    git.worktree_remove(repo_dir, worktree_dir, force)
        .await
        .map_err(|e| crate::error::Error::Isolation(format!("failed to remove worktree: {e}")))
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

/// Remove worktrees under `repo_dir/base_dir` whose directory mtime is older
/// than `max_age_days` days.
///
/// Returns the list of paths that were (or in dry-run mode, would have been)
/// removed. Errors from individual removals are silently skipped so a single
/// bad worktree does not abort the whole pass.
pub async fn cleanup_stale_worktrees(
    repo_dir: &Path,
    base_dir: &str,
    max_age_days: u64,
    dry_run: bool,
    git: &dyn GitClient,
) -> Vec<PathBuf> {
    let worktrees = list_worktrees(repo_dir, base_dir);
    let threshold = std::time::Duration::from_secs(max_age_days * 86_400);
    let now = std::time::SystemTime::now();

    let mut stale = Vec::new();
    for path in worktrees {
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
            .unwrap_or(std::time::Duration::ZERO);
        if age >= threshold {
            stale.push(path);
        }
    }

    if !dry_run {
        for path in &stale {
            if worktree_is_active(path) {
                tracing::info!(path = %path.display(), "skipping active worktree during cleanup");
                continue;
            }
            let _ = remove_worktree(repo_dir, path, true, git).await;
        }
    }

    stale
}

/// Validate that `repo_dir` is a local checkout of `repo` (owner/name).
///
/// Runs `git remote get-url origin` and checks that the output contains the
/// repo slug. Returns `Ok(())` on success, `Err(Error::Isolation(_))` if the
/// directory is not a git repo or the remote URL doesn't match.
pub async fn validate_repo_dir(
    repo_dir: &Path,
    repo: &str,
    git: &dyn GitClient,
) -> crate::error::Result<()> {
    let url = git.remote_url(repo_dir).await.map_err(|_| {
        crate::error::Error::Isolation(format!(
            "{} is not a git repository (or has no 'origin' remote)",
            repo_dir.display()
        ))
    })?;
    let url = url.trim();
    // The repo slug may appear as "owner/name", "owner/name.git", etc.
    let slug_bare = repo;
    let slug_git = format!("{repo}.git");
    if !url.contains(slug_bare) && !url.contains(&slug_git) {
        return Err(crate::error::Error::Isolation(format!(
            "{} remote origin URL '{url}' does not match repo '{repo}'",
            repo_dir.display()
        )));
    }

    Ok(())
}

/// Resolve the local directory for `repo`, optionally cloning if not found.
///
/// Resolution order:
/// 1. `explicit_dir` — if provided, validate it and return (or error).
/// 2. Current working directory — if it's a checkout of `repo`, use it.
/// 3. `~/.forza/repos/{repo}` — managed clone location, if it exists.
/// 4. Prompt the user to clone via `gh repo clone`; clone on confirmation.
pub async fn find_or_clone_repo(
    repo: &str,
    explicit_dir: Option<PathBuf>,
    git: &dyn GitClient,
) -> crate::error::Result<PathBuf> {
    // Step 1: explicit dir provided — validate and return.
    if let Some(dir) = explicit_dir {
        validate_repo_dir(&dir, repo, git).await?;
        return Ok(dir);
    }

    // Step 2: current working directory.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if validate_repo_dir(&cwd, repo, git).await.is_ok() {
        return Ok(cwd);
    }

    // Step 3: managed clone location.
    let managed = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".forza")
        .join("repos")
        .join(repo);
    if managed.exists() && validate_repo_dir(&managed, repo, git).await.is_ok() {
        return Ok(managed);
    }

    // Step 4: prompt to clone.
    eprint!(
        "Repository '{repo}' not found locally. Clone to {}? [y/N] ",
        managed.display()
    );

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| crate::error::Error::Isolation(format!("failed to read user input: {e}")))?;

    if !answer.trim().eq_ignore_ascii_case("y") {
        return Err(crate::error::Error::Isolation(format!(
            "repository '{repo}' not found locally and clone was declined"
        )));
    }

    // Create parent directory.
    if let Some(parent) = managed.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            crate::error::Error::Isolation(format!("failed to create {}: {e}", parent.display()))
        })?;
    }

    let output = tokio::process::Command::new("gh")
        .args(["repo", "clone", repo])
        .arg(&managed)
        .output()
        .await
        .map_err(|e| crate::error::Error::Isolation(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::Error::Isolation(format!(
            "gh repo clone failed: {stderr}"
        )));
    }

    Ok(managed)
}
