//! Configuration — the route-based config model.
//!
//! Each route maps a type (issue/pr) + label to a workflow template
//! with its own concurrency, polling frequency, and agent settings.

use std::collections::HashMap;
use std::path::Path;

use indexmap::IndexMap;

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

use tracing::{debug, warn};

use crate::error::{Error, Result};
use crate::github::{IssueCandidate, PrCandidate};

/// Per-repo entry in the multi-repo configuration table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoEntry {
    /// Local checkout path for this repo. Defaults to current directory.
    pub repo_dir: Option<String>,

    /// Routes specific to this repo.
    #[serde(default)]
    pub routes: IndexMap<String, Route>,
}

/// Top-level runner configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    /// Global settings.
    pub global: GlobalConfig,

    /// Per-repo configuration (multi-repo mode).
    /// When present, each key is a repo slug (`owner/name`) and each value
    /// holds that repo's optional local path and routes.
    #[serde(default)]
    pub repos: IndexMap<String, RepoEntry>,

    /// Security settings.
    #[serde(default)]
    pub security: SecurityConfig,

    /// Global validation commands (run between stages).
    #[serde(default)]
    pub validation: ValidationConfig,

    /// Named routes — the core routing table (legacy single-repo mode).
    #[serde(default)]
    pub routes: IndexMap<String, Route>,

    /// Custom workflow templates (override or extend built-ins).
    #[serde(default)]
    pub workflow_templates: Vec<crate::workflow::WorkflowTemplate>,

    /// Global agent config (skills, MCP, system prompt).
    #[serde(default)]
    pub agent_config: AgentConfig,

    /// Per-stage hooks.
    #[serde(default)]
    pub stage_hooks: HashMap<String, StageHooks>,

    /// Stage prompt template file overrides.
    #[serde(default)]
    pub prompt_templates: HashMap<String, String>,
}

/// Global settings shared across all routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Repository in `owner/name` format (legacy single-repo mode).
    /// Omit when using the `[repos]` table for multi-repo support.
    pub repo: Option<String>,

    /// Local checkout path. Default: current directory.
    pub repo_dir: Option<String>,

    /// Maximum concurrent runs across all routes.
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,

    /// Maximum cost per single issue (fail if exceeded).
    pub max_cost_per_issue: Option<f64>,

    /// Maximum hourly spend (pause all routes if exceeded).
    pub max_cost_per_hour: Option<f64>,

    /// Default agent.
    #[serde(default = "default_agent")]
    pub agent: String,

    /// Default model.
    pub model: Option<String>,

    /// Gate label — issues/PRs must have this label to be eligible.
    pub gate_label: Option<String>,

    /// Label applied when processing starts.
    #[serde(default = "default_in_progress_label")]
    pub in_progress_label: String,

    /// Label applied on successful completion.
    #[serde(default = "default_complete_label")]
    pub complete_label: String,

    /// Label applied on failure.
    #[serde(default = "default_failed_label")]
    pub failed_label: String,

    /// Branch naming pattern. {issue} and {slug} are replaced.
    #[serde(default = "default_branch_pattern")]
    pub branch_pattern: String,

    /// Stale lease timeout in seconds.
    #[serde(default = "default_stale_lease_timeout")]
    pub stale_lease_timeout: u64,

    /// Age threshold in days for automatic stale worktree cleanup.
    /// Worktrees older than this are removed by `forza clean --stale` and
    /// during watch mode. Default: 7.
    #[serde(default = "default_stale_worktree_days")]
    pub stale_worktree_days: u64,

    /// Default workflow when no route label matches.
    pub default_workflow: Option<String>,

    /// Whether to automatically merge PRs after CI passes. Default: false.
    #[serde(default)]
    pub auto_merge: bool,

    /// Whether to create a draft PR immediately after the plan stage. Default: false.
    #[serde(default)]
    pub draft_pr: bool,

    /// Notification settings. When absent, no notifications are sent.
    pub notifications: Option<NotificationsConfig>,

    /// GitHub API backend: "octocrab" (default) or "gh-cli".
    #[serde(default = "default_github_backend")]
    pub github_backend: String,

    /// Git backend: "gix" (default) or "git-cli".
    #[serde(default = "default_git_backend")]
    pub git_backend: String,
}

/// Notification channels fired on run completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    /// Send a desktop notification via `osascript` (macOS) or `notify-send` (Linux).
    #[serde(default)]
    pub desktop: bool,

    /// Slack incoming-webhook URL. When set, a formatted message is POSTed.
    pub slack_webhook: Option<String>,

    /// Generic webhook URL. When set, the run record is POSTed as JSON.
    pub webhook_url: Option<String>,
}

/// Security settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Authorization level: sandbox, local, contributor, trusted.
    #[serde(default = "default_auth_level")]
    pub authorization_level: String,

    /// Only process issues from these authors. Empty = authenticated user only.
    #[serde(default)]
    pub allowed_authors: Vec<String>,

    /// Additional label that must be present for an issue to be processed.
    /// Distinct from `gate_label` (which is the opt-in trigger); this label
    /// acts as a security gate checked after the issue is fetched.
    #[serde(default)]
    pub require_label: Option<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            authorization_level: default_auth_level(),
            allowed_authors: Vec::new(),
            require_label: None,
        }
    }
}

impl SecurityConfig {
    /// Returns `true` when the authorization level permits pushing a branch and opening a PR.
    ///
    /// Levels `"contributor"` and `"trusted"` allow push; `"sandbox"` and `"local"` do not.
    pub fn allows_push(&self) -> bool {
        matches!(self.authorization_level.as_str(), "contributor" | "trusted")
    }

    /// Returns `true` when the authorization level permits merging a PR.
    ///
    /// Only `"trusted"` allows merge.
    pub fn allows_merge(&self) -> bool {
        self.authorization_level == "trusted"
    }
}

/// Global validation commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationConfig {
    /// Commands to run between stages.
    #[serde(default)]
    pub commands: Vec<String>,
}

/// A time window during which a route is active.
///
/// Times are UTC in `"HH:MM"` format. If `end < start` the window spans midnight.
/// If `days` is empty, the window is active every day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleWindow {
    /// Days the route is active, e.g. `["Mon", "Tue", "Wed", "Thu", "Fri"]`.
    /// Empty (default) means all days.
    #[serde(default)]
    pub days: Vec<String>,

    /// Start of the active window in UTC, `"HH:MM"` format.
    pub start: String,

    /// End of the active window in UTC, `"HH:MM"` format.
    pub end: String,
}

impl ScheduleWindow {
    /// Returns `true` if `now` falls within this schedule window.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        // Check day constraint.
        if !self.days.is_empty() {
            let today = now.weekday();
            let day_match = self.days.iter().any(|d| {
                d.parse::<Weekday>()
                    .inspect_err(|_| warn!(value = %d, "failed to parse schedule window weekday"))
                    .map(|wd| wd == today)
                    .unwrap_or(false)
            });
            if !day_match {
                return false;
            }
        }

        let Ok(start) = NaiveTime::parse_from_str(&self.start, "%H:%M") else {
            warn!(value = %self.start, "failed to parse schedule window start time");
            return false;
        };
        let Ok(end) = NaiveTime::parse_from_str(&self.end, "%H:%M") else {
            warn!(value = %self.end, "failed to parse schedule window end time");
            return false;
        };

        let current = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0).unwrap_or(start);

        if end >= start {
            // Normal window: e.g. 09:00–17:00
            current >= start && current < end
        } else {
            // Overnight window: e.g. 22:00–06:00 (spans midnight)
            current >= start || current < end
        }
    }
}

/// A condition that triggers a route based on PR state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteCondition {
    /// CI checks are failing on the PR.
    CiFailing,
    /// The PR has merge conflicts.
    HasConflicts,
    /// CI is failing or the PR has conflicts (or both).
    CiFailingOrConflicts,
    /// The PR is approved and CI is green.
    ApprovedAndGreen,
    /// CI is green, no conflicts, and no CHANGES_REQUESTED review decision.
    /// Does not require an explicit APPROVED decision — branch protection is the real gate.
    CiGreenNoObjections,
    /// Matches any PR that needs attention: CI failing, has conflicts, or CI green with no
    /// objections (ready to merge). Combines `CiFailingOrConflicts` and `CiGreenNoObjections`
    /// into a single route to prevent ping-pong between two condition routes.
    AnyActionable,
}

impl RouteCondition {
    /// Evaluate whether this condition holds for the given PR.
    pub fn matches(&self, pr: &crate::github::PrCandidate) -> bool {
        let ci_failing = pr.checks_passing == Some(false);
        if let Some(m) = pr.mergeable.as_deref()
            && m != "MERGEABLE"
            && m != "CONFLICTING"
        {
            debug!(
                pr = pr.number,
                mergeable = m,
                "mergeability not yet resolved, skipping cycle"
            );
            return false;
        } else if pr.mergeable.is_none() {
            debug!(
                pr = pr.number,
                mergeable = "None",
                "mergeability not yet resolved, skipping cycle"
            );
            return false;
        }
        let has_conflicts = pr.mergeable.as_deref() == Some("CONFLICTING");
        let approved = pr.review_decision.as_deref() == Some("APPROVED");
        let changes_requested = pr.review_decision.as_deref() == Some("CHANGES_REQUESTED");
        let ci_green = pr.checks_passing == Some(true);

        match self {
            RouteCondition::CiFailing => ci_failing,
            RouteCondition::HasConflicts => has_conflicts,
            RouteCondition::CiFailingOrConflicts => ci_failing || has_conflicts,
            RouteCondition::ApprovedAndGreen => approved && ci_green && !has_conflicts,
            RouteCondition::CiGreenNoObjections => ci_green && !has_conflicts && !changes_requested,
            RouteCondition::AnyActionable => {
                ci_failing || has_conflicts || ci_green && !changes_requested
            }
        }
    }
}

/// Which PRs are in scope for condition evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionScope {
    /// Only PRs on branches created by forza (matching the branch pattern prefix).
    #[default]
    ForzaOwned,
    /// All open PRs in the repo.
    All,
}

/// The kind of subject a route operates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectType {
    /// GitHub issue.
    Issue,
    /// GitHub pull request.
    Pr,
}

/// A named route — maps type+trigger to an action.
///
/// A route must have at least one trigger (`label` or `condition`) and
/// at least one action (`workflow`). Label-triggered routes fire when a
/// subject has the specified label. Condition-triggered routes fire when
/// PR state matches (e.g., CI failing, has conflicts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// What kind of subject this route operates on.
    #[serde(rename = "type")]
    pub route_type: SubjectType,

    /// GitHub label that triggers this route.
    #[serde(default)]
    pub label: Option<String>,

    /// Condition that triggers this route when evaluated against a PR.
    #[serde(default)]
    pub condition: Option<RouteCondition>,

    /// Workflow template name to use.
    #[serde(default)]
    pub workflow: Option<String>,

    /// Maximum concurrent runs for this route.
    #[serde(default = "default_route_concurrency")]
    pub concurrency: usize,

    /// Poll interval in seconds for this route.
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,

    /// Override model for this route.
    pub model: Option<String>,

    /// Override skills for this route.
    pub skills: Option<Vec<String>>,

    /// Override validation commands for this route.
    pub validation_commands: Option<Vec<String>>,

    /// Override MCP config for this route.
    pub mcp_config: Option<String>,

    /// Optional schedule window. When set, issues for this route are only
    /// processed during the active window. `None` means always active.
    pub schedule: Option<ScheduleWindow>,

    /// Scope of PRs to evaluate conditions against.
    #[serde(default)]
    pub scope: ConditionScope,

    /// Maximum retry attempts for condition-triggered routes.
    /// When exceeded, `forza:needs-human` label is applied instead.
    pub max_retries: Option<usize>,

    /// Override draft_pr setting for this route.
    pub draft_pr: Option<bool>,

    /// Override branch_pattern for this route.
    pub branch_pattern: Option<String>,
}

impl Route {
    /// Validate that a route has at least one trigger and one action.
    pub fn validate(&self, name: &str) -> Result<()> {
        if self.label.is_some() && self.condition.is_some() {
            return Err(Error::Policy(format!(
                "route '{name}' cannot have both a label and a condition"
            )));
        }
        if self.label.is_none() && self.condition.is_none() {
            return Err(Error::Policy(format!(
                "route '{name}' must have a label or a condition"
            )));
        }
        if self.workflow.is_none() {
            return Err(Error::Policy(format!(
                "route '{name}' must have a workflow"
            )));
        }
        Ok(())
    }
}

/// Agent-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// Skill files to inject.
    #[serde(default)]
    pub skills: Vec<String>,

    /// MCP config file path.
    pub mcp_config: Option<String>,

    /// System prompt to append.
    pub append_system_prompt: Option<String>,
}

/// Pre/post/finally hooks for a specific stage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageHooks {
    #[serde(default)]
    pub pre: Vec<String>,
    #[serde(default)]
    pub post: Vec<String>,
    /// Runs after the stage regardless of success or failure.
    #[serde(default)]
    pub finally: Vec<String>,
}

// ── Defaults ────────────────────────────────────────────────────────

fn default_max_concurrency() -> usize {
    5
}

fn default_agent() -> String {
    "claude".to_string()
}

fn default_github_backend() -> String {
    "octocrab".to_string()
}

fn default_git_backend() -> String {
    "gix".to_string()
}

fn default_in_progress_label() -> String {
    "forza:in-progress".to_string()
}

fn default_complete_label() -> String {
    "forza:complete".to_string()
}

fn default_failed_label() -> String {
    "forza:failed".to_string()
}

fn default_branch_pattern() -> String {
    "automation/{issue}-{slug}".to_string()
}

fn default_stale_lease_timeout() -> u64 {
    3600
}

fn default_stale_worktree_days() -> u64 {
    7
}

fn default_auth_level() -> String {
    "contributor".to_string()
}

fn default_route_concurrency() -> usize {
    1
}

fn default_poll_interval() -> u64 {
    300
}

/// CLI-level overrides that take precedence over all config-file settings.
///
/// Built from `--model` and `--skill` flags and passed into the orchestrator so
/// that per-run CLI flags win over stage, route, and global config.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// Override the model for every stage in the run.
    pub model: Option<String>,
    /// Override the skill files for every stage in the run.
    pub skills: Vec<String>,
    /// Pre-matched route name (used by condition routes to skip label re-matching).
    pub route: Option<String>,
}

/// Overrides parsed from `forza:*` labels on an issue or PR.
///
/// Supported label prefixes:
/// - `forza:model:<name>` → override model (e.g., `forza:model:opus`)
/// - `forza:skill:<name>` → inject skill (e.g., `forza:skill:rust`)
///
/// Override chain: CLI > label > stage > route > global.
#[derive(Debug, Clone, Default)]
pub struct LabelOverrides {
    /// Model override from labels.
    pub model: Option<String>,
    /// Skill overrides from labels.
    pub skills: Vec<String>,
}

impl LabelOverrides {
    /// Parse overrides from a set of GitHub labels.
    pub fn from_labels(labels: &[String]) -> Self {
        let mut model = None;
        let mut skills = Vec::new();

        for label in labels {
            if let Some(m) = label.strip_prefix("forza:model:") {
                model = Some(m.to_string());
            } else if let Some(s) = label.strip_prefix("forza:skill:") {
                let path = std::path::Path::new(s);
                if path.is_absolute()
                    || path
                        .components()
                        .any(|c| c == std::path::Component::ParentDir)
                {
                    tracing::warn!(
                        label,
                        "ignoring skill label with unsafe path (absolute or traversal)"
                    );
                } else {
                    skills.push(s.to_string());
                }
            }
        }

        Self { model, skills }
    }

    /// Returns true if no overrides were found.
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.skills.is_empty()
    }
}

// ── Implementation ──────────────────────────────────────────────────

impl RunnerConfig {
    /// Load config from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content).map_err(|e| Error::Policy(e.to_string()))?;
        // Validate routes.
        for (name, route) in &config.routes {
            route.validate(name)?;
        }
        for entry in config.repos.values() {
            for (name, route) in &entry.routes {
                route.validate(name)?;
            }
        }
        Ok(config)
    }

    /// Iterate all repos yielding `(repo_slug, repo_dir, routes)` for each.
    ///
    /// In legacy single-repo mode (`repos` table absent), yields one entry from
    /// `global.repo` + top-level `routes`. In multi-repo mode, yields one entry
    /// per `[repos]` key.
    pub fn iter_repos(&self) -> Vec<(&str, Option<&str>, &IndexMap<String, Route>)> {
        if self.repos.is_empty() {
            let repo = self.global.repo.as_deref().unwrap_or("");
            let repo_dir = self.global.repo_dir.as_deref();
            vec![(repo, repo_dir, &self.routes)]
        } else {
            self.repos
                .iter()
                .map(|(repo, entry)| (repo.as_str(), entry.repo_dir.as_deref(), &entry.routes))
                .collect()
        }
    }

    /// Find the route matching an issue within the given route map.
    pub fn match_route_in<'a>(
        routes: &'a IndexMap<String, Route>,
        issue: &IssueCandidate,
    ) -> Option<(&'a str, &'a Route)> {
        for (name, route) in routes {
            if route.route_type == SubjectType::Issue
                && route
                    .label
                    .as_ref()
                    .is_some_and(|rl| issue.labels.iter().any(|l| l == rl))
            {
                return Some((name.as_str(), route));
            }
        }
        None
    }

    /// Find the route matching a PR within the given route map.
    pub fn match_pr_route_in<'a>(
        routes: &'a IndexMap<String, Route>,
        pr: &PrCandidate,
    ) -> Option<(&'a str, &'a Route)> {
        for (name, route) in routes {
            if route.route_type == SubjectType::Pr
                && route
                    .label
                    .as_ref()
                    .is_some_and(|rl| pr.labels.iter().any(|l| l == rl))
            {
                return Some((name.as_str(), route));
            }
        }
        None
    }

    /// Get the branch for a PR — uses the PR's existing head branch.
    pub fn branch_for_pr(pr: &PrCandidate) -> String {
        pr.head_branch.clone()
    }

    /// Get routes filtered by type ("issue" or "pr").
    pub fn routes_by_type(&self, route_type: SubjectType) -> Vec<(&str, &Route)> {
        self.routes
            .iter()
            .filter(|(_, r)| r.route_type == route_type)
            .map(|(name, route)| (name.as_str(), route))
            .collect()
    }

    /// Find the route that matches an issue's labels (legacy single-repo API).
    pub fn match_route(&self, issue: &IssueCandidate) -> Option<(&str, &Route)> {
        Self::match_route_in(&self.routes, issue)
    }

    /// Get the effective model for a route (route override > global default).
    pub fn effective_model<'a>(&'a self, route: &'a Route) -> Option<&'a str> {
        route.model.as_deref().or(self.global.model.as_deref())
    }

    /// Get the effective skills for a stage+route+global (stage > route > global).
    pub fn effective_skills<'a>(
        &'a self,
        route: &'a Route,
        stage_skills: Option<&'a [String]>,
    ) -> &'a [String] {
        if let Some(s) = stage_skills {
            return s;
        }
        if let Some(ref s) = route.skills {
            return s.as_slice();
        }
        &self.agent_config.skills
    }

    /// Get the effective MCP config for a stage+route+global (stage > route > global).
    pub fn effective_mcp_config<'a>(
        &'a self,
        route: &'a Route,
        stage_mcp: Option<&'a str>,
    ) -> Option<&'a str> {
        stage_mcp
            .or(route.mcp_config.as_deref())
            .or(self.agent_config.mcp_config.as_deref())
    }

    /// Get the global append_system_prompt from agent_config.
    pub fn effective_append_system_prompt(&self) -> Option<&str> {
        self.agent_config.append_system_prompt.as_deref()
    }

    /// Get the effective validation commands for a route.
    pub fn effective_validation<'a>(&'a self, route: &'a Route) -> &'a [String] {
        route
            .validation_commands
            .as_deref()
            .unwrap_or(&self.validation.commands)
    }

    /// Get the effective draft_pr setting (route override > global default).
    pub fn effective_draft_pr(&self, route: &Route) -> bool {
        route.draft_pr.unwrap_or(self.global.draft_pr)
    }

    /// Get the effective branch pattern for a route (route override > global default).
    pub fn effective_branch_pattern<'a>(&'a self, route: &'a Route) -> &'a str {
        route
            .branch_pattern
            .as_deref()
            .unwrap_or(&self.global.branch_pattern)
    }

    /// Collect all unique branch prefixes (the part before the first `{`) from
    /// the global `branch_pattern` and every route's optional `branch_pattern`
    /// override.  Used by the `forza_owned` scope filter in condition routes so
    /// that a condition route covers branches created by *any* forza route in
    /// the repo, not just the one that happens to share the global pattern.
    pub fn forza_owned_prefixes(&self, routes: &IndexMap<String, Route>) -> Vec<String> {
        let global_prefix = self
            .global
            .branch_pattern
            .split('{')
            .next()
            .unwrap_or("automation/")
            .to_string();
        let mut prefixes = vec![global_prefix];
        for route in routes.values() {
            if let Some(ref pattern) = route.branch_pattern {
                let prefix = pattern.split('{').next().unwrap_or("").to_string();
                if !prefix.is_empty() && !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
            }
        }
        prefixes
    }

    /// Generate a branch name for an issue.
    pub fn branch_for_issue(&self, issue: &IssueCandidate) -> String {
        let slug = slugify(&issue.title, 40);
        let branch = self
            .global
            .branch_pattern
            .replace("{issue}", &issue.number.to_string())
            .replace("{slug}", &slug);
        truncate_branch(&branch, 60)
    }

    /// Resolve a workflow template by name (custom overrides built-in).
    pub fn resolve_workflow(&self, name: &str) -> Option<crate::workflow::WorkflowTemplate> {
        // Check custom templates first.
        if let Some(template) = self.workflow_templates.iter().find(|t| t.name == name) {
            return Some(template.clone());
        }
        // Fall back to built-in.
        crate::workflow::builtin_templates()
            .into_iter()
            .find(|t| t.name == name)
    }
}

// ── Slug helpers (moved from policy.rs) ─────────────────────────────

fn slugify(title: &str, max_len: usize) -> String {
    let raw: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    let collapsed = collapse_hyphens(&raw);
    let trimmed = collapsed.trim_matches('-');

    trimmed
        .chars()
        .take(max_len)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

fn collapse_hyphens(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_hyphen = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result
}

fn truncate_branch(branch: &str, max_len: usize) -> String {
    if branch.chars().count() <= max_len {
        return branch.to_string();
    }
    branch
        .chars()
        .take(max_len)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_example_config() {
        let content = r#"
[global]
repo = "owner/repo"
max_concurrency = 3
model = "claude-sonnet-4-6"
gate_label = "runner:ready"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
concurrency = 1
poll_interval = 60

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
concurrency = 2
poll_interval = 300
"#;
        let config: RunnerConfig = toml::from_str(content).unwrap();
        assert_eq!(config.global.repo.as_deref(), Some("owner/repo"));
        assert_eq!(config.routes.len(), 2);
        assert_eq!(config.routes["bugfix"].workflow.as_deref(), Some("bug"));
        assert_eq!(config.routes["features"].concurrency, 2);
    }

    #[test]
    fn match_route_by_label() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();

        let issue = IssueCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "fix something".into(),
            body: "details here".into(),
            labels: vec!["bug".into()],
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            author: String::new(),
            comments: vec![],
        };

        let (name, route) = config.match_route(&issue).unwrap();
        assert_eq!(name, "bugfix");
        assert_eq!(route.workflow.as_deref(), Some("bug"));
    }

    #[test]
    fn no_route_match_returns_none() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();

        let issue = IssueCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "add feature".into(),
            body: "details".into(),
            labels: vec!["enhancement".into()],
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            author: String::new(),
            comments: vec![],
        };

        assert!(config.match_route(&issue).is_none());
    }

    #[test]
    fn effective_model_route_overrides_global() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
model = "sonnet"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
model = "opus"
"#,
        )
        .unwrap();

        assert_eq!(
            config.effective_model(&config.routes["bugfix"]),
            Some("opus")
        );
    }

    #[test]
    fn stage_hooks_finally_field() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[stage_hooks.implement]
pre = ["echo pre"]
post = ["cargo fmt --all"]
finally = ["echo done"]

[stage_hooks.test]
post = ["cargo test --lib"]
"#,
        )
        .unwrap();

        let implement = config.stage_hooks.get("implement").unwrap();
        assert_eq!(implement.pre, vec!["echo pre"]);
        assert_eq!(implement.post, vec!["cargo fmt --all"]);
        assert_eq!(implement.finally, vec!["echo done"]);

        let test_hooks = config.stage_hooks.get("test").unwrap();
        assert!(test_hooks.pre.is_empty());
        assert!(test_hooks.finally.is_empty());
    }

    #[test]
    fn auto_merge_defaults_to_false() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        assert!(!config.global.auto_merge);
    }

    #[test]
    fn auto_merge_can_be_set_to_true() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
auto_merge = true
"#,
        )
        .unwrap();
        assert!(config.global.auto_merge);
    }

    #[test]
    fn draft_pr_defaults_to_false() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        assert!(!config.global.draft_pr);
    }

    #[test]
    fn draft_pr_can_be_set_to_true() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
draft_pr = true
"#,
        )
        .unwrap();
        assert!(config.global.draft_pr);
    }

    #[test]
    fn effective_draft_pr_route_overrides_global() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
draft_pr = false

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
draft_pr = true
"#,
        )
        .unwrap();
        assert!(config.effective_draft_pr(&config.routes["bugfix"]));
    }

    #[test]
    fn effective_draft_pr_falls_back_to_global() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
draft_pr = true

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();
        assert!(config.effective_draft_pr(&config.routes["bugfix"]));
    }

    #[test]
    fn prompt_templates_parsed_from_toml() {
        let content = r#"
[global]
repo = "owner/repo"

[prompt_templates]
plan = "prompts/plan.md"
implement = "prompts/implement.md"
"#;
        let config: RunnerConfig = toml::from_str(content).unwrap();
        assert_eq!(
            config.prompt_templates.get("plan").map(|s| s.as_str()),
            Some("prompts/plan.md")
        );
        assert_eq!(
            config.prompt_templates.get("implement").map(|s| s.as_str()),
            Some("prompts/implement.md")
        );
        assert_eq!(config.prompt_templates.len(), 2);
    }

    #[test]
    fn slug_generation() {
        assert_eq!(slugify("fix: handle the thing", 40), "fix-handle-the-thing");
        assert_eq!(
            slugify("fix(scope): do something", 40),
            "fix-scope-do-something"
        );
        assert!(!slugify("  leading  ", 40).starts_with('-'));
        assert!(!slugify("  trailing  ", 40).ends_with('-'));
    }

    #[test]
    fn workflow_templates_parsed_from_toml() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[[workflow_templates]]
name = "quick-fix"

[[workflow_templates.stages]]
kind = "implement"

[[workflow_templates.stages]]
kind = "open_pr"
"#,
        )
        .unwrap();
        assert_eq!(config.workflow_templates.len(), 1);
        assert_eq!(config.workflow_templates[0].name, "quick-fix");
        assert_eq!(config.workflow_templates[0].stages.len(), 2);
    }

    #[test]
    fn resolve_workflow_returns_custom() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[[workflow_templates]]
name = "quick-fix"

[[workflow_templates.stages]]
kind = "implement"

[[workflow_templates.stages]]
kind = "open_pr"
"#,
        )
        .unwrap();
        let template = config.resolve_workflow("quick-fix").unwrap();
        assert_eq!(template.name, "quick-fix");
        assert_eq!(template.stages.len(), 2);
    }

    #[test]
    fn resolve_workflow_custom_overrides_builtin() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[[workflow_templates]]
name = "bug"

[[workflow_templates.stages]]
kind = "implement"
"#,
        )
        .unwrap();
        // The custom "bug" template has only 1 stage; the built-in has more.
        let template = config.resolve_workflow("bug").unwrap();
        assert_eq!(template.name, "bug");
        assert_eq!(template.stages.len(), 1);
    }

    #[test]
    fn resolve_workflow_falls_back_to_builtin() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        // No custom templates — should return the built-in "feature" template.
        let template = config.resolve_workflow("feature").unwrap();
        assert_eq!(template.name, "feature");
        assert!(!template.stages.is_empty());
    }

    #[test]
    fn schedule_window_parses_from_toml() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"

[routes.features.schedule]
days = ["Mon", "Tue", "Wed", "Thu", "Fri"]
start = "09:00"
end = "17:00"
"#,
        )
        .unwrap();

        let route = &config.routes["features"];
        let schedule = route.schedule.as_ref().unwrap();
        assert_eq!(schedule.days, vec!["Mon", "Tue", "Wed", "Thu", "Fri"]);
        assert_eq!(schedule.start, "09:00");
        assert_eq!(schedule.end, "17:00");
    }

    #[test]
    fn route_without_schedule_has_none() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();

        assert!(config.routes["bugfix"].schedule.is_none());
    }

    #[test]
    fn schedule_window_is_active_within_window() {
        let window = ScheduleWindow {
            days: vec![],
            start: "09:00".into(),
            end: "17:00".into(),
        };
        // 13:00 UTC — inside window
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(window.is_active(now));
    }

    #[test]
    fn schedule_window_is_inactive_outside_window() {
        let window = ScheduleWindow {
            days: vec![],
            start: "09:00".into(),
            end: "17:00".into(),
        };
        // 08:00 UTC — before window
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!window.is_active(now));

        // 18:00 UTC — after window
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!window.is_active(now));
    }

    #[test]
    fn schedule_window_overnight_active() {
        let window = ScheduleWindow {
            days: vec![],
            start: "22:00".into(),
            end: "06:00".into(),
        };
        // 23:00 — active (after start, before midnight)
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(window.is_active(now));

        // 03:00 — active (after midnight, before end)
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(window.is_active(now));
    }

    #[test]
    fn schedule_window_overnight_inactive() {
        let window = ScheduleWindow {
            days: vec![],
            start: "22:00".into(),
            end: "06:00".into(),
        };
        // 12:00 — inactive (middle of day)
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!window.is_active(now));
    }

    #[test]
    fn schedule_window_day_filter_active() {
        let window = ScheduleWindow {
            days: vec![
                "Mon".into(),
                "Tue".into(),
                "Wed".into(),
                "Thu".into(),
                "Fri".into(),
            ],
            start: "09:00".into(),
            end: "17:00".into(),
        };
        // 2024-03-20 is a Wednesday, 13:00 UTC
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-20T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(window.is_active(now));
    }

    #[test]
    fn schedule_window_day_filter_inactive_on_weekend() {
        let window = ScheduleWindow {
            days: vec![
                "Mon".into(),
                "Tue".into(),
                "Wed".into(),
                "Thu".into(),
                "Fri".into(),
            ],
            start: "09:00".into(),
            end: "17:00".into(),
        };
        // 2024-03-23 is a Saturday, 13:00 UTC
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-23T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!window.is_active(now));
    }

    #[test]
    fn multi_repo_config_parses() {
        let content = r#"
[global]
max_concurrency = 3
model = "claude-sonnet-4-6"

[repos."owner/repo-a"]
repo_dir = "/path/to/a"

[repos."owner/repo-a".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[repos."owner/repo-b".routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
"#;
        let config: RunnerConfig = toml::from_str(content).unwrap();
        assert!(config.global.repo.is_none());
        assert_eq!(config.repos.len(), 2);
        assert!(config.repos["owner/repo-a"].repo_dir.as_deref() == Some("/path/to/a"));
        assert_eq!(config.repos["owner/repo-a"].routes.len(), 1);
        assert_eq!(config.repos["owner/repo-b"].routes.len(), 1);
    }

    #[test]
    fn iter_repos_multi_mode() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
max_concurrency = 5

[repos."owner/alpha".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[repos."owner/beta".routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
"#,
        )
        .unwrap();

        let repos = config.iter_repos();
        assert_eq!(repos.len(), 2);
        let repo_names: Vec<&str> = repos.iter().map(|(r, _, _)| *r).collect();
        assert!(repo_names.contains(&"owner/alpha"));
        assert!(repo_names.contains(&"owner/beta"));
    }

    #[test]
    fn iter_repos_legacy_mode() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();

        let repos = config.iter_repos();
        assert_eq!(repos.len(), 1);
        let (repo, repo_dir, routes) = repos[0];
        assert_eq!(repo, "owner/repo");
        assert!(repo_dir.is_none());
        assert_eq!(routes.len(), 1);
    }

    #[test]
    fn match_route_in_finds_matching_route() {
        let mut routes = IndexMap::new();
        routes.insert(
            "bugfix".to_string(),
            Route {
                route_type: SubjectType::Issue,
                label: Some("bug".to_string()),
                workflow: Some("bug".to_string()),
                condition: None,
                concurrency: 1,
                poll_interval: 60,
                model: None,
                skills: None,
                validation_commands: None,
                mcp_config: None,
                schedule: None,
                scope: ConditionScope::default(),
                max_retries: None,
                draft_pr: None,
                branch_pattern: None,
            },
        );

        let issue = IssueCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "fix something".into(),
            body: "details".into(),
            labels: vec!["bug".into()],
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            author: String::new(),
            comments: vec![],
        };

        let (name, route) = RunnerConfig::match_route_in(&routes, &issue).unwrap();
        assert_eq!(name, "bugfix");
        assert_eq!(route.workflow.as_deref(), Some("bug"));
    }

    fn make_config_with_agent(
        global_skills: Vec<String>,
        global_mcp: Option<String>,
        route_skills: Option<Vec<String>>,
        route_mcp: Option<String>,
    ) -> (RunnerConfig, Route) {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();
        let mut config = config;
        config.agent_config.skills = global_skills;
        config.agent_config.mcp_config = global_mcp;
        let mut route = config.routes["bugfix"].clone();
        route.skills = route_skills;
        route.mcp_config = route_mcp;
        (config, route)
    }

    #[test]
    fn effective_skills_stage_overrides_route_and_global() {
        let (config, route) = make_config_with_agent(
            vec!["global.md".into()],
            None,
            Some(vec!["route.md".into()]),
            None,
        );
        let stage_skills = vec!["stage.md".into()];
        assert_eq!(
            config.effective_skills(&route, Some(&stage_skills)),
            &["stage.md"]
        );
    }

    #[test]
    fn effective_skills_route_overrides_global_when_no_stage() {
        let (config, route) = make_config_with_agent(
            vec!["global.md".into()],
            None,
            Some(vec!["route.md".into()]),
            None,
        );
        assert_eq!(config.effective_skills(&route, None), &["route.md"]);
    }

    #[test]
    fn effective_skills_falls_back_to_global_when_no_stage_or_route() {
        let (config, route) = make_config_with_agent(vec!["global.md".into()], None, None, None);
        assert_eq!(config.effective_skills(&route, None), &["global.md"]);
    }

    #[test]
    fn effective_mcp_config_stage_overrides_route_and_global() {
        let (config, route) = make_config_with_agent(
            vec![],
            Some("global.json".into()),
            None,
            Some("route.json".into()),
        );
        assert_eq!(
            config.effective_mcp_config(&route, Some("stage.json")),
            Some("stage.json")
        );
    }

    #[test]
    fn effective_mcp_config_route_overrides_global_when_no_stage() {
        let (config, route) = make_config_with_agent(
            vec![],
            Some("global.json".into()),
            None,
            Some("route.json".into()),
        );
        assert_eq!(
            config.effective_mcp_config(&route, None),
            Some("route.json")
        );
    }

    #[test]
    fn effective_mcp_config_falls_back_to_global_when_no_stage_or_route() {
        let (config, route) =
            make_config_with_agent(vec![], Some("global.json".into()), None, None);
        assert_eq!(
            config.effective_mcp_config(&route, None),
            Some("global.json")
        );
    }

    #[test]
    fn effective_mcp_config_none_when_all_absent() {
        let (config, route) = make_config_with_agent(vec![], None, None, None);
        assert_eq!(config.effective_mcp_config(&route, None), None);
    }

    #[test]
    fn resolve_workflow_unknown_returns_none() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        assert!(config.resolve_workflow("nonexistent").is_none());
    }

    #[test]
    fn route_validate_requires_trigger_and_action() {
        let route = Route {
            route_type: SubjectType::Pr,
            label: None,
            condition: None,
            workflow: Some("pr-fix".to_string()),
            concurrency: 1,
            poll_interval: 60,
            model: None,
            skills: None,
            validation_commands: None,
            mcp_config: None,
            schedule: None,
            scope: ConditionScope::default(),
            max_retries: None,
            draft_pr: None,
            branch_pattern: None,
        };
        assert!(route.validate("test").is_err()); // no trigger

        let route2 = Route {
            label: Some("bug".to_string()),
            workflow: None,
            ..route.clone()
        };
        assert!(route2.validate("test").is_err()); // no action

        let route3 = Route {
            label: Some("bug".to_string()),
            workflow: Some("bug".to_string()),
            ..route.clone()
        };
        assert!(route3.validate("test").is_ok()); // label + workflow

        let route4 = Route {
            condition: Some(RouteCondition::CiFailing),
            workflow: Some("pr-fix".to_string()),
            ..route.clone()
        };
        assert!(route4.validate("test").is_ok()); // condition + workflow

        let route5 = Route {
            label: Some("bug".to_string()),
            condition: Some(RouteCondition::CiFailing),
            workflow: Some("bug".to_string()),
            ..route
        };
        assert!(route5.validate("test").is_err()); // both label and condition
    }

    #[test]
    fn condition_route_parses_from_toml() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.auto-fix]
type = "pr"
condition = "ci_failing_or_conflicts"
workflow = "pr-fix"
scope = "forza_owned"
max_retries = 3
"#,
        )
        .unwrap();

        let route = &config.routes["auto-fix"];
        assert_eq!(route.condition, Some(RouteCondition::CiFailingOrConflicts));
        assert_eq!(route.workflow.as_deref(), Some("pr-fix"));
        assert_eq!(route.scope, ConditionScope::ForzaOwned);
        assert_eq!(route.max_retries, Some(3));
        assert!(route.label.is_none());
    }

    #[test]
    fn condition_matches_ci_failing() {
        let mut pr = crate::github::PrCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "test".into(),
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
        assert!(RouteCondition::CiFailing.matches(&pr));
        assert!(!RouteCondition::HasConflicts.matches(&pr));
        assert!(RouteCondition::CiFailingOrConflicts.matches(&pr));

        pr.checks_passing = Some(true);
        pr.mergeable = Some("CONFLICTING".into());
        assert!(!RouteCondition::CiFailing.matches(&pr));
        assert!(RouteCondition::HasConflicts.matches(&pr));
        assert!(RouteCondition::CiFailingOrConflicts.matches(&pr));

        pr.mergeable = Some("MERGEABLE".into());
        pr.review_decision = Some("APPROVED".into());
        assert!(RouteCondition::ApprovedAndGreen.matches(&pr));

        // UNKNOWN mergeability: transient GitHub state — must return false for all conditions
        pr.mergeable = Some("UNKNOWN".into());
        pr.checks_passing = Some(true);
        pr.review_decision = Some("APPROVED".into());
        assert!(!RouteCondition::HasConflicts.matches(&pr));
        assert!(!RouteCondition::CiFailingOrConflicts.matches(&pr));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
        assert!(!RouteCondition::AnyActionable.matches(&pr));

        // None mergeability: also returns false for all conditions
        pr.mergeable = None;
        assert!(!RouteCondition::HasConflicts.matches(&pr));
        assert!(!RouteCondition::CiFailingOrConflicts.matches(&pr));
        assert!(!RouteCondition::ApprovedAndGreen.matches(&pr));
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
        assert!(!RouteCondition::AnyActionable.matches(&pr));

        // CiGreenNoObjections: CI green + no review decision → matches
        pr.mergeable = Some("MERGEABLE".into());
        pr.checks_passing = Some(true);
        pr.review_decision = None;
        assert!(RouteCondition::CiGreenNoObjections.matches(&pr));

        // CiGreenNoObjections: CI green + CHANGES_REQUESTED → does not match
        pr.review_decision = Some("CHANGES_REQUESTED".into());
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));

        // CiGreenNoObjections: CI failing + no review decision → does not match
        pr.review_decision = None;
        pr.checks_passing = Some(false);
        assert!(!RouteCondition::CiGreenNoObjections.matches(&pr));
    }

    #[test]
    fn condition_matches_any_actionable() {
        let mut pr = crate::github::PrCandidate {
            number: 1,
            repo: "owner/repo".into(),
            title: "test".into(),
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
        // CI failing → matches
        assert!(RouteCondition::AnyActionable.matches(&pr));

        // Has conflicts → matches
        pr.checks_passing = Some(true);
        pr.mergeable = Some("CONFLICTING".into());
        assert!(RouteCondition::AnyActionable.matches(&pr));

        // CI green, no conflicts, no objections → matches (ready to merge)
        pr.mergeable = Some("MERGEABLE".into());
        pr.checks_passing = Some(true);
        pr.review_decision = None;
        assert!(RouteCondition::AnyActionable.matches(&pr));

        // CI green, no conflicts, but CHANGES_REQUESTED → does not match
        pr.review_decision = Some("CHANGES_REQUESTED".into());
        assert!(!RouteCondition::AnyActionable.matches(&pr));

        // CI not yet resolved (None), no conflicts → does not match
        pr.review_decision = None;
        pr.checks_passing = None;
        assert!(!RouteCondition::AnyActionable.matches(&pr));
    }

    #[test]
    fn security_config_allows_push_contributor() {
        let cfg = SecurityConfig {
            authorization_level: "contributor".into(),
            ..Default::default()
        };
        assert!(cfg.allows_push());
        assert!(!cfg.allows_merge());
    }

    #[test]
    fn security_config_allows_push_trusted() {
        let cfg = SecurityConfig {
            authorization_level: "trusted".into(),
            ..Default::default()
        };
        assert!(cfg.allows_push());
        assert!(cfg.allows_merge());
    }

    #[test]
    fn security_config_sandbox_blocks_all() {
        let cfg = SecurityConfig {
            authorization_level: "sandbox".into(),
            ..Default::default()
        };
        assert!(!cfg.allows_push());
        assert!(!cfg.allows_merge());
    }

    #[test]
    fn security_config_local_blocks_all() {
        let cfg = SecurityConfig {
            authorization_level: "local".into(),
            ..Default::default()
        };
        assert!(!cfg.allows_push());
        assert!(!cfg.allows_merge());
    }

    #[test]
    fn security_config_parsed_default_is_contributor() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        assert_eq!(config.security.authorization_level, "contributor");
        assert!(config.security.allows_push());
        assert!(!config.security.allows_merge());
    }

    #[test]
    fn label_overrides_parses_model() {
        let labels = vec![
            "bug".to_string(),
            "forza:ready".to_string(),
            "forza:model:opus".to_string(),
        ];
        let overrides = LabelOverrides::from_labels(&labels);
        assert_eq!(overrides.model.as_deref(), Some("opus"));
        assert!(overrides.skills.is_empty());
    }

    #[test]
    fn label_overrides_parses_skills() {
        let labels = vec![
            "forza:skill:rust".to_string(),
            "forza:skill:testing".to_string(),
        ];
        let overrides = LabelOverrides::from_labels(&labels);
        assert!(overrides.model.is_none());
        assert_eq!(overrides.skills, vec!["rust", "testing"]);
    }

    #[test]
    fn label_overrides_rejects_unsafe_skill_paths() {
        let labels = vec![
            "forza:skill:../etc/passwd".to_string(),
            "forza:skill:/absolute/path".to_string(),
            "forza:skill:skills/rust.md".to_string(),
        ];
        let overrides = LabelOverrides::from_labels(&labels);
        assert_eq!(overrides.skills, vec!["skills/rust.md"]);
    }

    #[test]
    fn label_overrides_empty_for_no_forza_labels() {
        let labels = vec!["bug".to_string(), "enhancement".to_string()];
        let overrides = LabelOverrides::from_labels(&labels);
        assert!(overrides.is_empty());
    }

    #[test]
    fn label_overrides_model_and_skills_together() {
        let labels = vec![
            "forza:model:haiku".to_string(),
            "forza:skill:rust".to_string(),
        ];
        let overrides = LabelOverrides::from_labels(&labels);
        assert_eq!(overrides.model.as_deref(), Some("haiku"));
        assert_eq!(overrides.skills, vec!["rust"]);
    }

    #[test]
    fn forza_owned_prefixes_global_only() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
branch_pattern = "automation/{issue}-{slug}"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap();

        let prefixes = config.forza_owned_prefixes(&config.routes);
        assert_eq!(prefixes, vec!["automation/"]);
    }

    #[test]
    fn forza_owned_prefixes_includes_route_overrides() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
branch_pattern = "automation/{issue}-{slug}"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
branch_pattern = "fix/{issue}-{slug}"
"#,
        )
        .unwrap();

        let prefixes = config.forza_owned_prefixes(&config.routes);
        assert!(prefixes.contains(&"automation/".to_string()));
        assert!(prefixes.contains(&"fix/".to_string()));
        assert_eq!(prefixes.len(), 2);
    }

    #[test]
    fn forza_owned_prefixes_deduplicates() {
        let config: RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
branch_pattern = "automation/{issue}-{slug}"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
branch_pattern = "automation/{issue}-{slug}"
"#,
        )
        .unwrap();

        let prefixes = config.forza_owned_prefixes(&config.routes);
        assert_eq!(prefixes, vec!["automation/"]);
    }
}
