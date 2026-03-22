//! forza — autonomous GitHub issue runner.
//!
//! Turns GitHub issues into pull requests through a staged pipeline.
//! Agent-agnostic design with three separated layers:
//!
//! ```text
//! GitHub (issues/PRs)  -->  Orchestrator  -->  Agent (claude/codex/etc)
//!     platform layer        domain layer        execution layer
//! ```
//!
//! # Architecture
//!
//! - **Platform** (`github`): Repository adapter for issues, PRs, comments, labels
//! - **Domain** (`planner`, `workflow`): Orchestration logic
//! - **Execution** (`executor`, `isolation`): Agent invocation and work isolation
//!
//! # Re-exports
//!
//! - [`RunnerConfig`]: top-level configuration loaded from `forza.toml`
//! - [`SubjectType`]: distinguishes issue routes (`SubjectType::Issue`) from PR routes (`SubjectType::Pr`)
//! - [`RouteOutcome`]: the final outcome recorded for a completed run
//! - [`process_pr_with_config`]: entry point for reactive PR processing

pub mod adapters;
pub mod api;
pub mod config;
pub mod deps;
pub mod error;
pub mod executor;
pub mod git;
pub mod github;
pub mod isolation;
pub mod mcp;
pub mod notifications;
pub mod orchestrator;
pub mod planner;
pub mod state;
pub mod workflow;

pub use config::{RunnerConfig, SubjectType};
pub use orchestrator::process_pr_with_config;
pub use state::RouteOutcome;
