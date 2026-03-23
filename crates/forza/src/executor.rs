//! Executor — agent-agnostic task execution.
//!
//! The `AgentAdapter` trait abstracts over different coding agents.
//! `ClaudeAdapter` implements it using claude-wrapper.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::planner::PlannedStage;
use crate::workflow::StageKind;

/// Result of executing a single stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: String,
    pub success: bool,
    pub duration_secs: f64,
    pub cost_usd: Option<f64>,
    pub output: String,
    pub files_modified: Option<u32>,
}

/// Agent adapter trait — implement for each supported agent.
pub trait AgentAdapter: Send + Sync {
    /// Execute a stage and return the result.
    fn execute_stage(
        &self,
        stage: &PlannedStage,
        work_dir: &Path,
    ) -> impl std::future::Future<Output = Result<StageResult>> + Send;
}

/// Claude Code adapter using claude-wrapper.
pub struct ClaudeAdapter {
    binary: Option<PathBuf>,
    model: Option<String>,
    max_turns: Option<u32>,
    skills: Vec<String>,
    mcp_config: Option<String>,
    append_system_prompt: Option<String>,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self {
            binary: None,
            model: None,
            max_turns: None,
            skills: Vec::new(),
            mcp_config: None,
            append_system_prompt: None,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn max_turns(mut self, turns: u32) -> Self {
        self.max_turns = Some(turns);
        self
    }

    pub fn binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.binary = Some(path.into());
        self
    }

    pub fn skills(mut self, skills: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.skills = skills.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn mcp_config(mut self, path: impl Into<String>) -> Self {
        self.mcp_config = Some(path.into());
        self
    }

    pub fn append_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.append_system_prompt = Some(prompt.into());
        self
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for ClaudeAdapter {
    async fn execute_stage(&self, stage: &PlannedStage, work_dir: &Path) -> Result<StageResult> {
        use claude_wrapper::streaming::{StreamEvent, stream_query};
        use claude_wrapper::{Claude, OutputFormat, PermissionMode, QueryCommand};

        let start = std::time::Instant::now();

        let mut builder = Claude::builder().working_dir(work_dir);
        if let Some(ref binary) = self.binary {
            builder = builder.binary(binary);
        }
        let claude = builder
            .build()
            .map_err(|e| Error::Executor(format!("failed to create claude client: {e}")))?;

        // Read skill files and prepend their content to the prompt instead of
        // using --file, which requires session token persistence in recent Claude
        // Code versions.
        let prompt = if self.skills.is_empty() {
            stage.prompt.clone()
        } else {
            let mut parts = Vec::with_capacity(self.skills.len() + 1);
            for skill in &self.skills {
                let skill_path = {
                    let p = Path::new(skill);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        work_dir.join(p)
                    }
                };
                match std::fs::read_to_string(&skill_path) {
                    Ok(content) => parts.push(content),
                    Err(e) => {
                        warn!(path = %skill_path.display(), error = %e, "skipping unreadable skill file");
                    }
                }
            }
            parts.push(stage.prompt.clone());
            parts.join("\n\n")
        };

        let mut cmd = QueryCommand::new(&prompt)
            .output_format(OutputFormat::StreamJson)
            .permission_mode(PermissionMode::BypassPermissions)
            .no_session_persistence();

        if let Some(ref model) = self.model {
            cmd = cmd.model(model);
        }
        if let Some(turns) = self.max_turns {
            cmd = cmd.max_turns(turns);
        }
        if let Some(ref p) = self.mcp_config {
            cmd = cmd.mcp_config(p);
        }
        if let Some(ref s) = self.append_system_prompt {
            cmd = cmd.append_system_prompt(s);
        }

        // Scope tools based on stage kind.
        match stage.kind {
            StageKind::Plan | StageKind::Research => {
                cmd = cmd.allowed_tools(["Read", "Glob", "Grep", "Write", "WebSearch", "WebFetch"]);
            }
            StageKind::Implement => {
                cmd = cmd.allowed_tools([
                    "Read",
                    "Edit",
                    "Write",
                    "Glob",
                    "Grep",
                    "Bash(cargo *)",
                    "Bash(npm *)",
                    "Bash(git *)",
                ]);
            }
            StageKind::Test => {
                cmd = cmd.allowed_tools([
                    "Read",
                    "Edit",
                    "Glob",
                    "Grep",
                    "Bash(cargo *)",
                    "Bash(npm *)",
                    "Bash(make *)",
                ]);
            }
            StageKind::Review => {
                cmd = cmd.allowed_tools(["Read", "Glob", "Grep"]);
                cmd = cmd.disallowed_tools(["Edit", "Write"]);
            }
            StageKind::Comment | StageKind::Clarify => {
                cmd = cmd.allowed_tools(["Read", "Glob", "Grep", "Bash(gh *)"]);
            }
            _ => {}
        }

        info!(
            stage = stage.kind_name(),
            work_dir = %work_dir.display(),
            "executing stage"
        );

        let mut captured_cost: Option<f64> = None;
        let output = stream_query(&claude, &cmd, |event: StreamEvent| {
            if let Some(t) = event.event_type() {
                debug!(stage = stage.kind_name(), event_type = t, "tool event");
            }
            if event.is_result()
                && let Some(cost) = event.cost_usd()
            {
                captured_cost = Some(cost);
            }
        })
        .await
        .map_err(|e| Error::Executor(format!("claude execution failed: {e}")))?;

        let duration = start.elapsed();
        let cost_usd = captured_cost;

        Ok(StageResult {
            stage: stage.kind_name().to_string(),
            success: output.success,
            duration_secs: duration.as_secs_f64(),
            cost_usd,
            output: if output.success {
                output.stdout
            } else {
                output.stderr
            },
            files_modified: None,
        })
    }
}
