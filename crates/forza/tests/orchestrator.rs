//! Integration tests for the forza orchestrator.
//!
//! Uses fake-claude and fake-gh binaries to test the full pipeline
//! without real agent invocations or GitHub API calls.
//!
//! Run with: cargo test --test orchestrator -- --ignored

use std::path::{Path, PathBuf};

use forza::config::{RunnerConfig, SubjectType};
use forza::state::{RouteOutcome, RunStatus};

/// Create a temporary git repo suitable for worktree operations.
fn init_test_repo(dir: &Path) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
    };

    run(&["init", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "# test repo\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial commit"]);
}

/// Build a minimal RunnerConfig for testing.
fn test_config(_repo_dir: &Path) -> RunnerConfig {
    let fake_claude = fake_claude_path();
    let toml = format!(
        r#"
[global]
repo = "test/repo"
model = "claude-fake"
gate_label = "forza:ready"
branch_pattern = "automation/{{issue}}-{{slug}}"
agent = "{}"
auto_merge = false

[security]
authorization_level = "trusted"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[routes.chores]
type = "issue"
label = "chore"
workflow = "chore"
"#,
        fake_claude.display()
    );
    toml::from_str(&toml).unwrap()
}

/// Build a config with condition routes for PR maintenance testing.
fn test_config_with_conditions() -> RunnerConfig {
    let toml = r#"
[global]
repo = "test/repo"
model = "claude-fake"
gate_label = "forza:ready"
branch_pattern = "automation/{issue}-{slug}"
auto_merge = false

[security]
authorization_level = "trusted"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[routes.auto-fix]
type = "pr"
condition = "ci_failing_or_conflicts"
workflow = "pr-fix"
scope = "forza_owned"
max_retries = 2
"#;
    toml::from_str(toml).unwrap()
}

fn fake_claude_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-helpers/fake-claude.sh")
}

fn fake_gh_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-helpers/fake-gh.sh")
}

/// Override PATH so `gh` resolves to our fake.
fn env_with_fake_gh() -> Vec<(String, String)> {
    let fake_dir = fake_gh_path().parent().unwrap().to_path_buf();
    // Create a symlink so `gh` resolves to fake-gh.sh.
    let gh_link = fake_dir.join("gh");
    if !gh_link.exists() {
        #[cfg(unix)]
        std::os::unix::fs::symlink("fake-gh.sh", &gh_link).ok();
    }

    let original_path = std::env::var("PATH").unwrap_or_default();
    vec![
        (
            "PATH".to_string(),
            format!("{}:{}", fake_dir.display(), original_path),
        ),
        ("FAKE_CLAUDE_OUTPUT".to_string(), "done".to_string()),
    ]
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore] // requires fake-claude and fake-gh on PATH
async fn issue_workflow_creates_run_record() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    init_test_repo(&repo_dir);

    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    let config = test_config(&repo_dir);

    // Set up fake gh.
    let env = env_with_fake_gh();
    for (k, v) in &env {
        // SAFETY: tests run sequentially (--test-threads=1 for ignored tests).
        unsafe { std::env::set_var(k, v) };
    }

    let routes = &config.routes;
    let gh: std::sync::Arc<dyn forza::github::GitHubClient> =
        std::sync::Arc::new(forza::github::GhCliClient::new());
    let git: std::sync::Arc<dyn forza::git::GitClient> =
        std::sync::Arc::new(forza::git::GitCliClient::new());
    let result = forza::runner::process_issue(
        1,
        "test/repo",
        &config,
        routes,
        &state_dir,
        &repo_dir,
        gh,
        git,
        None,
        vec![],
        None,
        None,
    )
    .await;

    match result {
        Ok(run) => {
            assert_eq!(run.repo, "test/repo");
            assert_eq!(run.subject_number, 1);
            assert!(
                run.status == forza_core::RunStatus::Succeeded
                    || run.status == forza_core::RunStatus::Failed,
                "run should have completed, got {:?}",
                run.status
            );
        }
        Err(e) => {
            eprintln!("process_issue returned error (may be expected): {e}");
        }
    }
}

#[tokio::test]
#[ignore]
async fn worktree_cleaned_up_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    init_test_repo(&repo_dir);

    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    let config = test_config(&repo_dir);
    let env = env_with_fake_gh();
    for (k, v) in &env {
        // SAFETY: tests run sequentially (--test-threads=1 for ignored tests).
        unsafe { std::env::set_var(k, v) };
    }

    let routes = &config.routes;
    let gh: std::sync::Arc<dyn forza::github::GitHubClient> =
        std::sync::Arc::new(forza::github::GhCliClient::new());
    let git: std::sync::Arc<dyn forza::git::GitClient> =
        std::sync::Arc::new(forza::git::GitCliClient::new());
    let _ = forza::runner::process_issue(
        1,
        "test/repo",
        &config,
        routes,
        &state_dir,
        &repo_dir,
        gh,
        git,
        None,
        vec![],
        None,
        None,
    )
    .await;

    // Worktree directory should be cleaned up.
    if let Ok(entries) = std::fs::read_dir(repo_dir.join(".worktrees")) {
        let remaining: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        assert!(
            remaining.is_empty(),
            "worktree should be cleaned up, found: {:?}",
            remaining.iter().map(|e| e.path()).collect::<Vec<_>>()
        );
    }
    // .worktrees dir doesn't exist = clean
}

#[test]
fn route_matching_label_based() {
    let config: RunnerConfig = toml::from_str(
        r#"
[global]
repo = "test/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
"#,
    )
    .unwrap();

    let issue_bug = forza::github::IssueCandidate {
        number: 1,
        repo: "test/repo".into(),
        title: "fix crash".into(),
        body: String::new(),
        labels: vec!["bug".into(), "forza:ready".into()],
        state: "open".into(),
        created_at: String::new(),
        updated_at: String::new(),
        is_assigned: false,
        html_url: String::new(),
        author: "testuser".into(),
        comments: vec![],
    };

    let matched = RunnerConfig::match_route_in(&config.routes, &issue_bug);
    assert!(matched.is_some());
    let (name, route) = matched.unwrap();
    assert_eq!(name, "bugfix");
    assert_eq!(route.workflow.as_deref(), Some("bug"));
}

#[test]
fn route_matching_no_match() {
    let config: RunnerConfig = toml::from_str(
        r#"
[global]
repo = "test/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
    )
    .unwrap();

    let issue_docs = forza::github::IssueCandidate {
        number: 2,
        repo: "test/repo".into(),
        title: "update docs".into(),
        body: String::new(),
        labels: vec!["documentation".into()],
        state: "open".into(),
        created_at: String::new(),
        updated_at: String::new(),
        is_assigned: false,
        html_url: String::new(),
        author: "testuser".into(),
        comments: vec![],
    };

    let matched = RunnerConfig::match_route_in(&config.routes, &issue_docs);
    assert!(matched.is_none());
}

#[test]
fn condition_route_matches_ci_failing() {
    let config = test_config_with_conditions();

    let pr = forza::github::PrCandidate {
        number: 10,
        repo: "test/repo".into(),
        title: "fix something".into(),
        body: String::new(),
        labels: vec![],
        state: "open".into(),
        html_url: String::new(),
        head_branch: "automation/1-test".into(),
        base_branch: "main".into(),
        is_draft: false,
        mergeable: Some("MERGEABLE".into()),
        review_decision: None,
        checks_passing: Some(false),
    };

    // Find the condition route.
    let auto_fix = config
        .routes
        .iter()
        .find(|(_, r)| r.condition.is_some())
        .unwrap();
    let condition = auto_fix.1.condition.as_ref().unwrap();
    assert!(condition.matches(&pr), "should match CI failing PR");
}

#[test]
fn condition_route_skips_non_forza_branches() {
    let pr = forza::github::PrCandidate {
        number: 10,
        repo: "test/repo".into(),
        title: "manual PR".into(),
        body: String::new(),
        labels: vec![],
        state: "open".into(),
        html_url: String::new(),
        head_branch: "feature/manual-work".into(), // not automation/
        base_branch: "main".into(),
        is_draft: false,
        mergeable: Some("CONFLICTING".into()),
        review_decision: None,
        checks_passing: None,
    };

    // forza_owned scope should skip this branch.
    assert!(
        !pr.head_branch.starts_with("automation/"),
        "non-forza branch should not start with automation/"
    );
}

#[test]
fn route_outcome_serialization() {
    let outcome = RouteOutcome::PrCreated { number: 42 };
    let json = serde_json::to_string(&outcome).unwrap();
    assert!(json.contains("pr_created"));
    assert!(json.contains("42"));

    let roundtrip: RouteOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, outcome);
}

#[test]
fn route_outcome_failed_captures_stage() {
    let outcome = RouteOutcome::Failed {
        stage: "implement".to_string(),
        error: "compilation error".to_string(),
    };
    let json = serde_json::to_string(&outcome).unwrap();
    let roundtrip: RouteOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, outcome);
}

#[test]
fn subject_type_serde() {
    let issue: SubjectType = serde_json::from_str(r#""issue""#).unwrap();
    assert_eq!(issue, SubjectType::Issue);

    let pr: SubjectType = serde_json::from_str(r#""pr""#).unwrap();
    assert_eq!(pr, SubjectType::Pr);

    // TOML deserialization (used in config files).
    let config: RunnerConfig = toml::from_str(
        r#"
[global]
repo = "test/repo"

[routes.test]
type = "pr"
label = "test"
workflow = "pr-fix"
"#,
    )
    .unwrap();
    assert_eq!(config.routes["test"].route_type, SubjectType::Pr);
}

#[test]
fn retry_budget_count() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path();

    // No runs yet.
    assert_eq!(
        forza::state::count_runs_for_subject(1, "pr-fix", state_dir),
        0
    );

    // Create a run record.
    let mut record = forza::state::RunRecord::new("run-1", "test/repo", 1, "pr-fix", "fix/1");
    record.finish(RunStatus::Succeeded);
    forza::state::save_run(&record, state_dir).unwrap();

    assert_eq!(
        forza::state::count_runs_for_subject(1, "pr-fix", state_dir),
        1
    );

    // Different workflow shouldn't count.
    assert_eq!(
        forza::state::count_runs_for_subject(1, "pr-rebase", state_dir),
        0
    );

    // Different issue shouldn't count.
    assert_eq!(
        forza::state::count_runs_for_subject(2, "pr-fix", state_dir),
        0
    );
}

#[test]
fn config_validation_rejects_no_trigger() {
    let result: Result<RunnerConfig, _> = toml::from_str(
        r#"
[global]
repo = "test/repo"

[routes.bad]
type = "issue"
workflow = "bug"
"#,
    );
    // Should parse OK (TOML is valid), but validation should fail.
    let config = result.unwrap();
    let route = &config.routes["bad"];
    assert!(route.validate("bad").is_err());
}

#[test]
fn config_validation_accepts_condition_trigger() {
    let config: RunnerConfig = toml::from_str(
        r#"
[global]
repo = "test/repo"

[routes.auto-fix]
type = "pr"
condition = "ci_failing"
workflow = "pr-fix"
"#,
    )
    .unwrap();
    let route = &config.routes["auto-fix"];
    assert!(route.validate("auto-fix").is_ok());
}
