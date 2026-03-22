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
pub mod traits;

// ── Re-exports ──────────────────────────────────────────────────────────

pub use condition::RouteCondition;
pub use error::{Error, Result};
pub use route::{MatchedWork, Route, Scope, Trigger};
pub use run::{generate_run_id, Outcome, Run, RunStatus, StageRecord, StageResult, StageStatus};
pub use stage::{Execution, Stage, StageKind, Workflow};
pub use subject::{Subject, SubjectKind};
pub use traits::{AgentExecutor, GitClient, GitHubClient};
