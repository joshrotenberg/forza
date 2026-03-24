//! forza-core — core abstractions for the forza workflow orchestrator.
//!
//! This crate defines the domain model that the `forza` binary crate builds on.
//! It contains no CLI, HTTP, or agent-specific code — only the types, traits,
//! and logic that describe how subjects are discovered, matched to routes,
//! executed through workflows, and recorded.
//!
//! # Architecture
//!
//! ```text
//! Subject ──→ Route ──→ Workflow ──→ [Stage, Stage, ...] ──→ Run
//!   │            │          │              │                    │
//!   │            │          │              ├─ Agent execution   │
//!   │            │          │              └─ Shell execution   │
//!   │            │          │                                   │
//!   │            │          └─ always linear, no branching      │
//!   │            └─ label or condition trigger                  │
//!   └─ issue or PR from GitHub                                 └─ persistent record
//! ```
//!
//! # Key Design Decisions
//!
//! - **One route, one action.** Routes do exactly one thing. Multi-step PR
//!   maintenance uses multiple routes across poll cycles.
//! - **Match once, carry through.** A subject is bound to its route at discovery
//!   time. No re-matching during execution.
//! - **GitHub is the state machine.** PR state on GitHub is authoritative. Routes
//!   are transition functions, the poll loop is the event loop.
//! - **Everything is public and serializable.** REST, MCP, CLI, and metrics can
//!   access any piece of data without going through accessors.
//!
//! # Modules
//!
//! | Module | Primary exports | Purpose |
//! |--------|----------------|---------|
//! | [`condition`] | [`RouteCondition`] | PR-state conditions that trigger condition routes |
//! | [`error`] | [`Error`], [`Result`] | Crate-wide error type and result alias |
//! | [`lifecycle`] | [`lifecycle::LifecycleLabels`] | `forza:*` label management (in-progress, complete, failed, needs-human) |
//! | [`pipeline`] | [`pipeline::PipelineConfig`], [`pipeline::execute`] | Unified stage-by-stage execution path for all subjects |
//! | [`planner`] | [`planner::generate_prompts`] | Build per-stage prompts from a subject and workflow |
//! | [`route`] | [`Route`], [`Trigger`], [`Scope`], [`MatchedWork`] | Route definitions and subject-matching logic |
//! | [`run`] | [`Run`], [`RunStatus`], [`Outcome`], [`StageRecord`] | Persistent run records and outcome tracking |
//! | [`shell`] | [`shell::ShellResult`], [`shell::run`] | `sh -c` execution with `FORZA_*` environment variables |
//! | [`stage`] | [`Stage`], [`StageKind`], [`Workflow`], [`Execution`] | Stage and workflow type definitions |
//! | [`subject`] | [`Subject`], [`SubjectKind`] | Unified GitHub issue/PR type flowing through the pipeline |
//! | [`traits`] | [`GitHubClient`], [`GitClient`], [`AgentExecutor`] | Pluggable backend traits implemented by the `forza` binary |

/// Mock implementations of forza-core traits for testing.
/// Available unconditionally so integration tests and downstream crates can use them.
pub mod testing;

pub mod condition;
pub mod error;
pub mod lifecycle;
pub mod pipeline;
pub mod planner;
pub mod route;
pub mod run;
pub mod shell;
pub mod stage;
pub mod subject;
pub mod tools;
pub mod traits;

// ── Re-exports ──────────────────────────────────────────────────────────

pub use condition::RouteCondition;
pub use error::{Error, Result};
pub use route::{MatchedWork, Route, Scope, Trigger};
pub use run::{Outcome, Run, RunStatus, StageRecord, StageResult, StageStatus, generate_run_id};
pub use stage::{Execution, Stage, StageKind, Workflow};
pub use subject::{Subject, SubjectKind};
pub use traits::{AgentExecutor, GitClient, GitHubClient};
