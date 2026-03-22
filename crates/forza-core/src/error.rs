//! Error types for forza-core.
//!
//! All errors are public and serializable so they can be surfaced through
//! REST, MCP, or CLI interfaces without lossy conversion.

use thiserror::Error;

/// Top-level error type for forza-core operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Route matching failed — no route handles this subject.
    #[error("no matching route: {0}")]
    NoMatchingRoute(String),

    /// Workflow resolution failed — the route references an unknown workflow.
    #[error("unknown workflow: {0}")]
    UnknownWorkflow(String),

    /// A stage failed during execution.
    #[error("stage failed: {stage}: {reason}")]
    StageFailed {
        /// Which stage failed.
        stage: String,
        /// Why it failed.
        reason: String,
    },

    /// Security policy blocked the operation.
    #[error("policy violation: {0}")]
    Policy(String),

    /// GitHub API error.
    #[error("github: {0}")]
    GitHub(String),

    /// Git operation error.
    #[error("git: {0}")]
    Git(String),

    /// Agent execution error.
    #[error("agent: {0}")]
    Agent(String),

    /// Shell command error (hooks, conditions, agentless stages).
    #[error("shell: {0}")]
    Shell(String),

    /// Worktree / isolation error.
    #[error("isolation: {0}")]
    Isolation(String),

    /// Configuration error.
    #[error("config: {0}")]
    Config(String),

    /// State persistence error.
    #[error("state: {0}")]
    State(String),

    /// IO error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias used throughout forza-core.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_context() {
        let e = Error::StageFailed {
            stage: "implement".into(),
            reason: "compilation failed".into(),
        };
        assert_eq!(e.to_string(), "stage failed: implement: compilation failed");
    }

    #[test]
    fn error_no_matching_route() {
        let e = Error::NoMatchingRoute("PR #42 has no matching labels".into());
        assert!(e.to_string().contains("PR #42"));
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e: Error = io_err.into();
        assert!(e.to_string().contains("file missing"));
    }
}
