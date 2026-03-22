//! Shell command execution with standardized environment variables.
//!
//! All shell commands (agentless stages, conditions, hooks, validation) go
//! through this module. Every invocation gets the full set of `FORZA_*`
//! environment variables so commands can reference the subject without
//! hardcoding.

use std::path::Path;
use std::time::Instant;

use tracing::debug;

use crate::subject::Subject;

/// Result of running a shell command.
#[derive(Debug, Clone)]
pub struct ShellResult {
    /// Whether the command exited with code 0.
    pub success: bool,
    /// Exit code, if available.
    pub exit_code: Option<i32>,
    /// Combined stdout/stderr output.
    pub output: String,
    /// Wall-clock duration.
    pub duration: std::time::Duration,
}

/// Run a shell command via `sh -c` with forza environment variables.
///
/// Sets all `FORZA_*` env vars from the subject, plus run/route/workflow context.
pub async fn run(
    command: &str,
    work_dir: &Path,
    subject: &Subject,
    run_id: &str,
    route: &str,
    workflow: &str,
) -> ShellResult {
    let start = Instant::now();
    let env_vars = subject.env_vars(run_id, route, workflow);

    debug!(
        command,
        work_dir = %work_dir.display(),
        "executing shell command"
    );

    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", command]).current_dir(work_dir);
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }

    let result = cmd.output().await;
    let duration = start.elapsed();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{stdout}\n{stderr}")
            };

            ShellResult {
                success: output.status.success(),
                exit_code: output.status.code(),
                output: combined,
                duration,
            }
        }
        Err(e) => ShellResult {
            success: false,
            exit_code: None,
            output: e.to_string(),
            duration,
        },
    }
}

/// Evaluate a condition command. Returns `true` if the command exits 0.
pub async fn check_condition(
    command: &str,
    work_dir: &Path,
    subject: &Subject,
    run_id: &str,
    route: &str,
    workflow: &str,
) -> bool {
    let result = run(command, work_dir, subject, run_id, route, workflow).await;
    result.success
}

/// Run multiple commands sequentially. Returns on first failure.
pub async fn run_all(
    commands: &[String],
    work_dir: &Path,
    subject: &Subject,
    run_id: &str,
    route: &str,
    workflow: &str,
) -> std::result::Result<(), (String, ShellResult)> {
    for cmd in commands {
        let result = run(cmd, work_dir, subject, run_id, route, workflow).await;
        if !result.success {
            return Err((cmd.clone(), result));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subject::{Subject, SubjectKind};

    fn make_subject() -> Subject {
        Subject {
            kind: SubjectKind::Issue,
            number: 42,
            repo: "owner/repo".into(),
            title: "Test".into(),
            body: String::new(),
            labels: vec![],
            html_url: String::new(),
            author: "user".into(),
            branch: "automation/42-test".into(),
            mergeable: None,
            checks_passing: None,
            review_decision: None,
            is_draft: None,
            base_branch: None,
        }
    }

    #[tokio::test]
    async fn run_simple_command() {
        let subject = make_subject();
        let result = run("echo hello", Path::new("/tmp"), &subject, "run-1", "test", "bug").await;
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn run_failing_command() {
        let subject = make_subject();
        let result = run("exit 1", Path::new("/tmp"), &subject, "run-1", "test", "bug").await;
        assert!(!result.success);
        assert_eq!(result.exit_code, Some(1));
    }

    #[tokio::test]
    async fn env_vars_are_set() {
        let subject = make_subject();
        let result = run(
            "echo $FORZA_SUBJECT_NUMBER",
            Path::new("/tmp"),
            &subject,
            "run-1",
            "bugfix",
            "bug",
        )
        .await;
        assert!(result.success);
        assert!(result.output.trim() == "42", "got: {}", result.output);
    }

    #[tokio::test]
    async fn issue_number_env_var() {
        let subject = make_subject();
        let result = run(
            "echo $FORZA_ISSUE_NUMBER",
            Path::new("/tmp"),
            &subject,
            "run-1",
            "bugfix",
            "bug",
        )
        .await;
        assert!(result.success);
        assert_eq!(result.output.trim(), "42");
    }

    #[tokio::test]
    async fn pr_number_env_var() {
        let mut subject = make_subject();
        subject.kind = SubjectKind::Pr;
        let result = run(
            "echo $FORZA_PR_NUMBER",
            Path::new("/tmp"),
            &subject,
            "run-1",
            "auto-merge",
            "pr-merge",
        )
        .await;
        assert!(result.success);
        assert_eq!(result.output.trim(), "42");
    }

    #[tokio::test]
    async fn check_condition_true() {
        let subject = make_subject();
        let result =
            check_condition("true", Path::new("/tmp"), &subject, "run-1", "test", "bug").await;
        assert!(result);
    }

    #[tokio::test]
    async fn check_condition_false() {
        let subject = make_subject();
        let result =
            check_condition("false", Path::new("/tmp"), &subject, "run-1", "test", "bug").await;
        assert!(!result);
    }

    #[tokio::test]
    async fn run_all_stops_on_failure() {
        let subject = make_subject();
        let commands = vec![
            "echo first".to_string(),
            "exit 1".to_string(),
            "echo third".to_string(),
        ];
        let result = run_all(
            &commands,
            Path::new("/tmp"),
            &subject,
            "run-1",
            "test",
            "bug",
        )
        .await;
        assert!(result.is_err());
        let (failed_cmd, _) = result.unwrap_err();
        assert_eq!(failed_cmd, "exit 1");
    }

    #[tokio::test]
    async fn run_all_succeeds_when_all_pass() {
        let subject = make_subject();
        let commands = vec!["true".to_string(), "true".to_string()];
        let result = run_all(
            &commands,
            Path::new("/tmp"),
            &subject,
            "run-1",
            "test",
            "bug",
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn duration_is_measured() {
        let subject = make_subject();
        let result =
            run("sleep 0.01", Path::new("/tmp"), &subject, "run-1", "test", "bug").await;
        assert!(result.duration.as_millis() >= 10);
    }
}
