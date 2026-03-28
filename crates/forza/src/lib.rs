//! forza — autonomous GitHub issue runner.
//!
//! Turns GitHub issues into pull requests through a staged pipeline.
//! Core domain logic lives in `forza-core`; this crate provides the CLI,
//! REST API, MCP server, and concrete implementations.
//!
//! ```text
//! GitHub (issues/PRs)  -->  Runner  -->  Agent (claude/codex/etc)
//!     platform layer       pipeline       execution layer
//! ```
//!
//! # Architecture
//!
//! - **Platform** (`github`, `git`): GitHub and git client implementations
//! - **Runner** (`runner`): Discovery, scheduling, and pipeline execution via `forza-core`
//! - **Execution** (`executor`, `isolation`): Agent invocation and work isolation
//! - **Adapters** (`adapters`): Bridge existing clients to `forza-core` traits
//!
//! # Re-exports
//!
//! - [`RunnerConfig`]: top-level configuration loaded from `forza.toml`
//! - [`SubjectType`]: distinguishes issue routes from PR routes
//! - [`RouteOutcome`]: the final outcome recorded for a completed run

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
pub mod plan;
pub mod planner;
pub mod runner;
pub mod state;
pub mod workflow;

pub use config::{RunnerConfig, SubjectType};
pub use state::RouteOutcome;
