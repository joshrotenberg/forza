//! Git operations — trait abstraction over git CLI and gix.
//!
//! The `GitClient` trait abstracts all git operations forza needs.
//! Implementations:
//! - `GitCliClient` — shells out to the `git` CLI
//! - `GixClient` — uses the gix crate (pure Rust)

mod cli;
mod gix_client;

pub use cli::GitCliClient;
pub use gix_client::GixClient;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::Result;

/// Trait abstracting all git operations forza needs.
///
/// All operations take a `repo_dir` parameter — the path to the
/// main repository checkout. Worktree operations create/remove
/// additional checkouts relative to this directory.
#[async_trait]
pub trait GitClient: Send + Sync {
    /// Fetch from origin.
    async fn fetch(&self, repo_dir: &Path) -> Result<()>;

    /// Create a worktree for a branch. If the branch doesn't exist,
    /// create it from `origin/main` (or the default branch).
    /// Returns the path to the worktree directory.
    async fn worktree_add(
        &self,
        repo_dir: &Path,
        branch: &str,
        worktree_base: &str,
    ) -> Result<PathBuf>;

    /// Remove a worktree directory.
    async fn worktree_remove(
        &self,
        repo_dir: &Path,
        worktree_dir: &Path,
        force: bool,
    ) -> Result<()>;

    /// Get the remote URL for "origin".
    async fn remote_url(&self, repo_dir: &Path) -> Result<String>;

    /// Check if a git ref exists (branch, tag, or commit).
    async fn ref_exists(&self, repo_dir: &Path, ref_name: &str) -> Result<bool>;

    /// Check if there are uncommitted changes (tracked files only).
    async fn has_changes(&self, work_dir: &Path) -> Result<bool>;

    /// Stage all tracked file changes.
    async fn stage_tracked(&self, work_dir: &Path) -> Result<()>;

    /// Stage a specific path (tracked or untracked).
    async fn stage_path(&self, work_dir: &Path, path: &str) -> Result<()>;

    /// Create a commit with the given message.
    async fn commit(&self, work_dir: &Path, message: &str) -> Result<()>;

    /// Rebase the current branch onto a ref. Returns true if successful.
    async fn rebase(&self, work_dir: &Path, onto: &str) -> Result<bool>;

    /// Abort an in-progress rebase.
    async fn rebase_abort(&self, work_dir: &Path) -> Result<()>;

    /// Get diff stat relative to a ref.
    async fn diff_stat(&self, work_dir: &Path, base: &str) -> Result<String>;

    /// Push a branch to origin.
    async fn push(&self, work_dir: &Path, branch: &str) -> Result<()>;

    /// Push a branch to origin with --force-with-lease.
    async fn push_force(&self, work_dir: &Path, branch: &str) -> Result<()>;

    /// Check if git is available and return version string.
    async fn version(&self) -> Result<String>;
}
