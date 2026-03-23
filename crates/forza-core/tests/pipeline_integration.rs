//! Integration tests for the forza-core pipeline using mock traits.
//!
//! These tests exercise `pipeline::execute` end-to-end with MockGitHub,
//! MockGit, and MockAgent — no real API calls, instant execution.

use forza_core::lifecycle::LifecycleLabels;
use forza_core::pipeline::{self, PipelineConfig};
use forza_core::route::MatchedWork;
use forza_core::run::{Outcome, RunStatus, StageStatus};
use forza_core::stage::{Stage, StageKind, Workflow};
use forza_core::testing::*;
use forza_core::{Route, Scope, Trigger};

use std::collections::HashMap;

fn default_config() -> PipelineConfig {
    PipelineConfig {
        labels: LifecycleLabels::default(),
        model: Some("test-model".into()),
        skills: vec![],
        mcp_config: None,
        validation: vec![],
        append_system_prompt: None,
        stage_hooks: HashMap::new(),
    }
}

fn bug_route() -> Route {
    Route {
        subject_type: forza_core::SubjectKind::Issue,
        trigger: Trigger::Label("bug".into()),
        workflow: "bug".into(),
        scope: Scope::ForzaOwned,
        concurrency: 1,
        poll_interval: 60,
        max_retries: None,
        model: None,
        skills: None,
        mcp_config: None,
        validation_commands: None,
    }
}

fn make_work(issue_number: u64) -> MatchedWork {
    MatchedWork {
        subject: make_test_issue(issue_number, "Fix the bug", &["bug", "forza:ready"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    }
}

// ── Happy path ──────────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_all_agent_stages_succeed() {
    let gh = MockGitHub::new().with_issue(1, "Fix bug", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new();

    let workflow = Workflow::new(
        "test",
        vec![
            Stage::agent(StageKind::Plan),
            Stage::agent(StageKind::Implement),
            Stage::agent(StageKind::Test),
        ],
    );

    let work = make_work(1);
    let prompts = vec![
        "plan prompt".into(),
        "implement prompt".into(),
        "test prompt".into(),
    ];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.stages.len(), 3);
    assert!(
        run.stages
            .iter()
            .all(|s| s.status == StageStatus::Succeeded)
    );
    assert_eq!(agent.call_count(), 3);

    // Lifecycle labels: in-progress added then removed, complete added.
    assert!(gh.label_was_added(1, "forza:in-progress"));
    assert!(gh.label_was_removed(1, "forza:in-progress"));
    assert!(gh.label_was_added(1, "forza:complete"));
}

// ── Stage failure ───────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_stops_on_required_stage_failure() {
    let gh = MockGitHub::new().with_issue(2, "Broken", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new().on_prompt_containing("implement", failure_result("compile error"));

    let workflow = Workflow::new(
        "test",
        vec![
            Stage::agent(StageKind::Plan),
            Stage::agent(StageKind::Implement),
            Stage::agent(StageKind::Test), // should not run
        ],
    );

    let work = MatchedWork {
        subject: make_test_issue(2, "Broken", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["plan".into(), "implement".into(), "test".into()];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.stages.len(), 2); // plan + implement, test never ran
    assert_eq!(run.stages[0].status, StageStatus::Succeeded);
    assert_eq!(run.stages[1].status, StageStatus::Failed);
    assert_eq!(agent.call_count(), 2);

    // Failed label should be set.
    assert!(gh.label_was_added(2, "forza:failed"));
}

#[tokio::test]
async fn pipeline_continues_past_optional_failure() {
    let gh = MockGitHub::new().with_issue(3, "Optional fail", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new().on_prompt_containing("test", failure_result("flaky test"));

    let workflow = Workflow::new(
        "test",
        vec![
            Stage::agent(StageKind::Plan),
            Stage::agent(StageKind::Test).optional(), // fails but optional
            Stage::agent(StageKind::Review),          // should still run
        ],
    );

    let work = MatchedWork {
        subject: make_test_issue(3, "Optional fail", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["plan".into(), "test".into(), "review".into()];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    // Overall succeeds because the failed stage was optional.
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.stages.len(), 3);
    assert_eq!(run.stages[1].status, StageStatus::Failed); // test failed
    assert_eq!(run.stages[2].status, StageStatus::Succeeded); // review still ran
    assert_eq!(agent.call_count(), 3);
}

// ── Shell stages ────────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_shell_stage_uses_env_vars() {
    let gh = MockGitHub::new().with_issue(42, "Env test", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new();

    let workflow = Workflow::new(
        "test",
        vec![Stage::shell(StageKind::Merge, "echo $FORZA_SUBJECT_NUMBER")],
    )
    .without_worktree();

    let work = MatchedWork {
        subject: make_test_issue(42, "Env test", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["".into()]; // shell stages get empty prompts
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Succeeded);
    let output = run.stages[0].result.as_ref().unwrap().output.trim();
    assert_eq!(output, "42");
}

// ── Worktree ────────────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_fails_on_worktree_creation_error() {
    let gh = MockGitHub::new().with_issue(5, "Wt fail", &["bug"]);
    let git = MockGit::new().fail_worktree();
    let agent = MockAgent::new();

    let workflow = Workflow::new("test", vec![Stage::agent(StageKind::Plan)]);

    let work = MatchedWork {
        subject: make_test_issue(5, "Wt fail", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["plan".into()];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Failed);
    assert!(matches!(
        run.outcome,
        Some(Outcome::Failed { ref stage, .. }) if stage == "setup"
    ));
    assert_eq!(agent.call_count(), 0); // agent never called
}

#[tokio::test]
async fn pipeline_skips_worktree_when_not_needed() {
    let gh = MockGitHub::new().with_issue(6, "No wt", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new();

    let workflow = Workflow::new("test", vec![Stage::shell(StageKind::Merge, "echo merged")])
        .without_worktree();

    let work = MatchedWork {
        subject: make_test_issue(6, "No wt", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["".into()];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Succeeded);
    // Git should not have been asked to create a worktree.
    let git_calls = git.calls.lock().unwrap();
    assert!(
        !git_calls
            .iter()
            .any(|c| matches!(c, MockCall::CreateWorktree(_))),
        "should not create worktree"
    );
}

// ── Validation ──────────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_stops_on_validation_failure() {
    let gh = MockGitHub::new().with_issue(7, "Val fail", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new();

    let workflow = Workflow::new(
        "test",
        vec![
            Stage::agent(StageKind::Plan),
            Stage::agent(StageKind::Implement), // should not run
        ],
    );

    let work = MatchedWork {
        subject: make_test_issue(7, "Val fail", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["plan".into(), "implement".into()];
    let mut config = default_config();
    config.validation = vec!["false".to_string()]; // always fails

    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Failed);
    // Plan succeeded, but validation after plan failed, so implement never ran.
    assert_eq!(agent.call_count(), 1);
}

// ── Lifecycle labels ────────────────────────────────────────────────────

#[tokio::test]
async fn pipeline_lifecycle_labels_on_success() {
    let gh = MockGitHub::new().with_issue(8, "Labels", &["bug", "forza:ready"]);
    let git = MockGit::new();
    let agent = MockAgent::new();

    let workflow = Workflow::new("test", vec![Stage::shell(StageKind::Merge, "true")]);

    let mut work = make_work(8);
    work.subject = make_test_issue(8, "Labels", &["bug", "forza:ready"]);
    let prompts = vec!["".into()];
    let mut config = default_config();
    config.labels.gate = Some("forza:ready".into());

    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert!(gh.label_was_added(8, "forza:in-progress"));
    assert!(gh.label_was_removed(8, "forza:ready")); // gate label removed
    assert!(gh.label_was_removed(8, "forza:in-progress"));
    assert!(gh.label_was_added(8, "forza:complete"));
}

#[tokio::test]
async fn pipeline_lifecycle_labels_on_failure() {
    let gh = MockGitHub::new().with_issue(9, "Fail", &["bug"]);
    let git = MockGit::new();
    let agent = MockAgent::new().always_fail("boom");

    let workflow = Workflow::new("test", vec![Stage::agent(StageKind::Implement)]);

    let work = MatchedWork {
        subject: make_test_issue(9, "Fail", &["bug"]),
        route_name: "bugfix".into(),
        route: bug_route(),
        workflow_name: "test".into(),
    };
    let prompts = vec!["implement".into()];
    let config = default_config();
    let tmp = tempfile::tempdir().unwrap();

    let run = pipeline::execute(
        &work,
        &workflow,
        &config,
        tmp.path(),
        tmp.path(),
        &gh,
        &git,
        &agent,
        &prompts,
    )
    .await;

    assert_eq!(run.status, RunStatus::Failed);
    assert!(gh.label_was_added(9, "forza:in-progress"));
    assert!(gh.label_was_removed(9, "forza:in-progress"));
    assert!(gh.label_was_added(9, "forza:failed"));
    assert!(!gh.label_was_added(9, "forza:complete"));
}
