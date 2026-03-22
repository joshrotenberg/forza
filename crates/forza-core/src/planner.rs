//! Planner — generate stage prompts from a subject and workflow.
//!
//! The planner takes a [`Subject`] and a [`Workflow`] and produces a `Vec<String>`
//! of prompts — one per stage. Agent stages get a generated prompt with all
//! variables substituted. Shell stages get an empty string (the command is in
//! the stage definition).
//!
//! Prompt templates are `.md` files loaded at compile time via `include_str!`.
//! Each template uses `{variable}` placeholders that are substituted based on
//! the subject type (issue or PR).

use crate::stage::{Execution, StageKind, Workflow};
use crate::subject::{Subject, SubjectKind};

// ── Embedded prompt templates ───────────────────────────────────────────

const PROMPT_PLAN: &str = include_str!("prompts/plan.md");
const PROMPT_IMPLEMENT: &str = include_str!("prompts/implement.md");
const PROMPT_TEST: &str = include_str!("prompts/test.md");
const PROMPT_REVIEW: &str = include_str!("prompts/review.md");
const PROMPT_OPEN_PR: &str = include_str!("prompts/open_pr.md");
const PROMPT_CLARIFY: &str = include_str!("prompts/clarify.md");
const PROMPT_RESEARCH: &str = include_str!("prompts/research.md");
const PROMPT_COMMENT: &str = include_str!("prompts/comment.md");
const PROMPT_PR_FIX_CI: &str = include_str!("prompts/pr_fix_ci.md");
const PROMPT_PR_REVISE: &str = include_str!("prompts/pr_revise_pr.md");
const PROMPT_PR_REVIEW: &str = include_str!("prompts/pr_review.md");
const _PROMPT_PR_MERGE: &str = include_str!("prompts/pr_merge.md");

/// Generate prompts for each stage in a workflow.
///
/// Returns a `Vec<String>` with one prompt per stage, in the same order
/// as `workflow.stages`. Shell (agentless) stages get empty prompts.
///
/// # Arguments
///
/// * `subject` — The issue or PR being processed.
/// * `workflow` — The workflow to generate prompts for.
/// * `run_id` — Used for breadcrumb paths.
/// * `validation_commands` — Validation commands to include in relevant prompts.
/// * `preamble` — Optional preamble prepended to all agent prompts.
pub fn generate_prompts(
    subject: &Subject,
    workflow: &Workflow,
    run_id: &str,
    validation_commands: &[String],
    preamble: &str,
) -> Vec<String> {
    let stage_count = workflow.stages.len();
    workflow
        .stages
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            // Shell stages don't need prompts — the command is in the stage.
            if matches!(stage.execution, Execution::Shell { .. }) {
                return String::new();
            }

            let has_successor = i < stage_count - 1;
            let template = select_template(stage.kind, subject.kind);
            substitute(
                template,
                subject,
                run_id,
                validation_commands,
                preamble,
                has_successor,
            )
        })
        .collect()
}

/// Select the appropriate prompt template for a stage kind and subject type.
fn select_template(kind: StageKind, subject_kind: SubjectKind) -> &'static str {
    match (kind, subject_kind) {
        // Issue-specific stages
        (StageKind::Plan, SubjectKind::Issue) => PROMPT_PLAN,
        (StageKind::Implement, SubjectKind::Issue) => PROMPT_IMPLEMENT,
        (StageKind::Test, _) => PROMPT_TEST,
        (StageKind::Review, SubjectKind::Issue) => PROMPT_REVIEW,
        (StageKind::OpenPr, _) => PROMPT_OPEN_PR,
        (StageKind::Clarify, _) => PROMPT_CLARIFY,
        (StageKind::Research, _) => PROMPT_RESEARCH,
        (StageKind::Comment, _) => PROMPT_COMMENT,

        // PR-specific stages
        (StageKind::FixCi, _) => PROMPT_PR_FIX_CI,
        (StageKind::RevisePr, _) => PROMPT_PR_REVISE,
        (StageKind::Review, SubjectKind::Pr) => PROMPT_PR_REVIEW,
        (StageKind::Plan, SubjectKind::Pr) => PROMPT_PLAN,
        (StageKind::Implement, SubjectKind::Pr) => PROMPT_IMPLEMENT,

        // Merge and triage use minimal prompts
        (StageKind::Merge, _) => "Merge the PR.",
        (StageKind::Triage, _) => "Triage the issue.",
    }
}

/// Substitute variables in a prompt template.
///
/// Handles both issue and PR variables. Missing variables are left as-is
/// (the template may contain variables not relevant to this subject type).
fn substitute(
    template: &str,
    subject: &Subject,
    run_id: &str,
    validation_commands: &[String],
    preamble: &str,
    has_successor: bool,
) -> String {
    let validation_step = if validation_commands.is_empty() {
        String::new()
    } else {
        let cmds = validation_commands
            .iter()
            .map(|c| format!("   - `{c}`"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n. Run validation commands to verify your changes:\n{cmds}\n   Fix any failures before proceeding.\n"
        )
    };

    // Breadcrumb instruction for stages with successors
    let breadcrumb_instruction = if has_successor {
        format!(
            "\n\n## Breadcrumb\n\nWhen done, write a brief context summary to \
             `.forza/breadcrumbs/{run_id}/{stage}.md`. Include key decisions and \
             any information the next stage will need.",
            stage = "current" // The actual stage name would be set by the caller
        )
    } else {
        String::new()
    };

    // Security-wrap user-provided content
    let issue_body_wrapped = format!(
        "--- BEGIN USER-PROVIDED CONTENT ---\n{}\n--- END USER-PROVIDED CONTENT ---",
        subject.body
    );

    let issue_context = format!("Title: {}\n\n{}", subject.title, issue_body_wrapped);

    let commit_num = if validation_commands.is_empty() {
        "\n4"
    } else {
        "\n5"
    };

    template
        .replace("{preamble}", preamble)
        .replace("{issue_number}", &subject.number.to_string())
        .replace("{pr_number}", &subject.number.to_string())
        .replace("{issue_title}", &subject.title)
        .replace("{pr_title}", &subject.title)
        .replace("{issue_body}", &issue_body_wrapped)
        .replace("{pr_body}", &issue_body_wrapped)
        .replace("{issue_context}", &issue_context)
        .replace("{branch}", &subject.branch)
        .replace("{head_branch}", &subject.branch)
        .replace(
            "{base_branch}",
            subject.base_branch.as_deref().unwrap_or("main"),
        )
        .replace("{repo}", &subject.repo)
        .replace("{run_id}", run_id)
        .replace("{validation_step}", &validation_step)
        .replace("{commit_num}", commit_num)
        .replace("{breadcrumb_instruction}", &breadcrumb_instruction)
}

/// Generate a project-scoped preamble for agent prompts.
///
/// Uses the repo name to scope the agent's work to the correct project.
pub fn make_preamble(repo: &str) -> String {
    let project = repo.rsplit('/').next().unwrap_or(repo);
    format!(
        "You are an automation agent working exclusively on the **{project}** project \
         ({repo}). Only read, modify, and create files within this repository. \
         Do not perform general internet research or make changes outside the project scope."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::{Stage, Workflow};

    fn make_issue() -> Subject {
        Subject {
            kind: SubjectKind::Issue,
            number: 42,
            repo: "owner/repo".into(),
            title: "Fix the bug".into(),
            body: "It's broken. Please fix.".into(),
            labels: vec!["bug".into()],
            html_url: String::new(),
            author: "user".into(),
            branch: "automation/42-fix-the-bug".into(),
            mergeable: None,
            checks_passing: None,
            review_decision: None,
            is_draft: None,
            base_branch: None,
        }
    }

    fn make_pr() -> Subject {
        Subject {
            kind: SubjectKind::Pr,
            number: 99,
            repo: "owner/repo".into(),
            title: "fix: the bug".into(),
            body: "Fixes #42".into(),
            labels: vec![],
            html_url: String::new(),
            author: "user".into(),
            branch: "automation/42-fix-the-bug".into(),
            mergeable: Some("MERGEABLE".into()),
            checks_passing: Some(true),
            review_decision: None,
            is_draft: Some(false),
            base_branch: Some("main".into()),
        }
    }

    #[test]
    fn generate_prompts_matches_stage_count() {
        let subject = make_issue();
        let workflow = Workflow::new(
            "bug",
            vec![
                Stage::agent(StageKind::Plan),
                Stage::agent(StageKind::Implement),
                Stage::agent(StageKind::Test),
            ],
        );
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert_eq!(prompts.len(), 3);
    }

    #[test]
    fn shell_stages_get_empty_prompts() {
        let subject = make_issue();
        let workflow = Workflow::new(
            "test",
            vec![
                Stage::agent(StageKind::Plan),
                Stage::shell(StageKind::Merge, "gh pr merge"),
            ],
        );
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(!prompts[0].is_empty()); // agent stage
        assert!(prompts[1].is_empty()); // shell stage
    }

    #[test]
    fn issue_prompts_contain_issue_number() {
        let subject = make_issue();
        let workflow = Workflow::new("bug", vec![Stage::agent(StageKind::Plan)]);
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(
            prompts[0].contains("#42"),
            "prompt should contain issue number: {}",
            &prompts[0][..200.min(prompts[0].len())]
        );
    }

    #[test]
    fn pr_prompts_contain_pr_number() {
        let subject = make_pr();
        let workflow = Workflow::new("pr-fix-ci", vec![Stage::agent(StageKind::FixCi)]);
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(
            prompts[0].contains("#99") || prompts[0].contains("99"),
            "prompt should contain PR number"
        );
    }

    #[test]
    fn preamble_is_substituted() {
        let subject = make_issue();
        let workflow = Workflow::new("bug", vec![Stage::agent(StageKind::Plan)]);
        let preamble = "You are working on forza.";
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], preamble);
        assert!(prompts[0].contains("You are working on forza."));
    }

    #[test]
    fn validation_commands_included() {
        let subject = make_issue();
        let workflow = Workflow::new("bug", vec![Stage::agent(StageKind::Implement)]);
        let validation = vec!["cargo fmt --check".to_string(), "cargo test".to_string()];
        let prompts = generate_prompts(&subject, &workflow, "run-1", &validation, "");
        assert!(prompts[0].contains("cargo fmt --check"));
        assert!(prompts[0].contains("cargo test"));
    }

    #[test]
    fn user_content_is_security_wrapped() {
        let subject = make_issue();
        let workflow = Workflow::new("bug", vec![Stage::agent(StageKind::Plan)]);
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(prompts[0].contains("BEGIN USER-PROVIDED CONTENT"));
        assert!(prompts[0].contains("END USER-PROVIDED CONTENT"));
    }

    #[test]
    fn make_preamble_uses_repo_name() {
        let preamble = make_preamble("joshrotenberg/forza");
        assert!(preamble.contains("**forza**"));
        assert!(preamble.contains("joshrotenberg/forza"));
    }

    #[test]
    fn make_preamble_handles_bare_name() {
        let preamble = make_preamble("forza");
        assert!(preamble.contains("**forza**"));
    }

    #[test]
    fn branch_substituted_in_pr_prompts() {
        let subject = make_pr();
        let workflow = Workflow::new("pr-fix-ci", vec![Stage::agent(StageKind::FixCi)]);
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(
            prompts[0].contains("automation/42-fix-the-bug"),
            "PR prompt should contain branch name"
        );
    }

    #[test]
    fn repo_substituted_in_prompts() {
        let subject = make_pr();
        let workflow = Workflow::new("pr-fix-ci", vec![Stage::agent(StageKind::FixCi)]);
        let prompts = generate_prompts(&subject, &workflow, "run-1", &[], "");
        assert!(prompts[0].contains("owner/repo"));
    }

    #[test]
    fn all_builtin_workflows_produce_prompts() {
        let issue = make_issue();
        let pr = make_pr();
        for wf in Workflow::builtins() {
            let subject = if wf.name.starts_with("pr-") {
                &pr
            } else {
                &issue
            };
            let prompts = generate_prompts(subject, &wf, "run-1", &[], "");
            assert_eq!(
                prompts.len(),
                wf.stages.len(),
                "prompt count mismatch for workflow '{}'",
                wf.name
            );
        }
    }
}
