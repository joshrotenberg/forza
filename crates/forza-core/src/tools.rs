//! Tool list selection for agent stages.
//!
//! Mirrors the same three-tier resolution as `planner::select_template`:
//! 1. `{tools_dir}/{agent}/{stage}.txt` — agent-specific override
//! 2. `{tools_dir}/{stage}.txt` — generic override
//! 3. compiled-in builtin
//!
//! Each `.txt` file contains one tool name per line. Empty lines and lines
//! starting with `#` are ignored. Example:
//!
//! ```text
//! Read
//! Edit
//! Write
//! Bash(cargo *)
//! ```
//!
//! To override tools for a specific stage, place a `.txt` file in the
//! `tools/` directory of the repo (parallel to `prompts/`).

use crate::stage::StageKind;

// ── Embedded builtin tool lists ──────────────────────────────────────────

const TOOLS_PLAN: &str = include_str!("tools/plan.txt");
const TOOLS_IMPLEMENT: &str = include_str!("tools/implement.txt");
const TOOLS_TEST: &str = include_str!("tools/test.txt");
const TOOLS_REVIEW: &str = include_str!("tools/review.txt");
const TOOLS_OPEN_PR: &str = include_str!("tools/open_pr.txt");
const TOOLS_CLARIFY: &str = include_str!("tools/clarify.txt");
const TOOLS_RESEARCH: &str = include_str!("tools/research.txt");
const TOOLS_COMMENT: &str = include_str!("tools/comment.txt");
const TOOLS_FIX_CI: &str = include_str!("tools/fix_ci.txt");
const TOOLS_REVISE_PR: &str = include_str!("tools/revise_pr.txt");

/// Return the compiled-in builtin tool list for a stage kind.
fn builtin_tools(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Plan => TOOLS_PLAN,
        StageKind::Implement => TOOLS_IMPLEMENT,
        StageKind::Test => TOOLS_TEST,
        StageKind::Review => TOOLS_REVIEW,
        StageKind::OpenPr => TOOLS_OPEN_PR,
        StageKind::Clarify => TOOLS_CLARIFY,
        StageKind::Research => TOOLS_RESEARCH,
        StageKind::Comment => TOOLS_COMMENT,
        StageKind::FixCi => TOOLS_FIX_CI,
        StageKind::RevisePr => TOOLS_REVISE_PR,
        // Agentless or no-restriction stages return empty — no tool scoping applied.
        StageKind::Merge | StageKind::Triage | StageKind::DraftPr => "",
    }
}

/// Parse a newline-separated tool list, ignoring empty lines and `#` comments.
fn parse_tools(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Select the allowed tools for a stage.
///
/// Resolution order:
/// 1. `{tools_dir}/{agent}/{stage}.txt` — agent-specific override
/// 2. `{tools_dir}/{stage}.txt` — generic override
/// 3. compiled-in builtin
///
/// Returns an empty `Vec` for agentless stages (Merge, Triage, DraftPr).
pub fn select_tools(
    kind: StageKind,
    agent: &str,
    tools_dir: Option<&std::path::Path>,
) -> Vec<String> {
    let filename = kind.name();

    if let Some(dir) = tools_dir {
        let agent_path = dir.join(agent).join(format!("{filename}.txt"));
        if let Ok(content) = std::fs::read_to_string(&agent_path) {
            return parse_tools(&content);
        }
        let generic_path = dir.join(format!("{filename}.txt"));
        if let Ok(content) = std::fs::read_to_string(&generic_path) {
            return parse_tools(&content);
        }
    }

    parse_tools(builtin_tools(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_plan_includes_read_and_write() {
        let tools = select_tools(StageKind::Plan, "claude", None);
        assert!(tools.contains(&"Read".to_string()));
        assert!(tools.contains(&"Write".to_string()));
    }

    #[test]
    fn builtin_review_is_read_only() {
        let tools = select_tools(StageKind::Review, "claude", None);
        assert!(tools.contains(&"Read".to_string()));
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
    }

    #[test]
    fn builtin_implement_includes_bash() {
        let tools = select_tools(StageKind::Implement, "claude", None);
        assert!(tools.iter().any(|t| t.starts_with("Bash")));
    }

    #[test]
    fn agentless_stages_return_empty() {
        assert!(select_tools(StageKind::Merge, "claude", None).is_empty());
        assert!(select_tools(StageKind::DraftPr, "claude", None).is_empty());
        assert!(select_tools(StageKind::Triage, "claude", None).is_empty());
    }

    #[test]
    fn parse_tools_ignores_empty_lines_and_comments() {
        let content = "Read\n# a comment\n\nWrite\n";
        let tools = parse_tools(content);
        assert_eq!(tools, vec!["Read", "Write"]);
    }

    #[test]
    fn override_from_generic_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plan.txt"), "CustomTool\n").unwrap();
        let tools = select_tools(StageKind::Plan, "claude", Some(dir.path()));
        assert_eq!(tools, vec!["CustomTool"]);
    }

    #[test]
    fn agent_override_takes_precedence_over_generic() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("claude");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("plan.txt"), "AgentTool\n").unwrap();
        std::fs::write(dir.path().join("plan.txt"), "GenericTool\n").unwrap();
        let tools = select_tools(StageKind::Plan, "claude", Some(dir.path()));
        assert_eq!(tools, vec!["AgentTool"]);
    }

    #[test]
    fn falls_back_to_builtin_when_dir_has_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let tools = select_tools(StageKind::Plan, "claude", Some(dir.path()));
        assert!(!tools.is_empty());
        assert!(tools.contains(&"Read".to_string()));
    }
}
