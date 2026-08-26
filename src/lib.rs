//! Mooshik — an ambient, local-first AI cowork partner and workspace orchestrator.
//!
//! Authority: `docs/SPEC.md`; build order: `dev-diary/PLAN.md`.
//!
//! Module layout grows with the milestones rather than being scaffolded up front
//! (a skeleton written before there is anything to link against gets rewritten):
//!
//! | Module      | Milestone | Contents                                     |
//! | ----------- | --------- | -------------------------------------------- |
//! | `cli`       | M0+       | argument parsing; a subcommand per milestone |
//! | `text`      | all       | user-facing strings from TOML, not inline    |
//! | `config`    | M1        | config.toml load/merge/validate, env overlay |
//! | `home`      | M1        | `~/.mooshik` layout, first-run creation      |
//! | `memory`    | M2        | `lambo::Memory` wiring, session endpoint     |
//! | `companion` | M3        | OpenAI-compatible /v1 client, chat loop      |
//! | `tools`     | M4        | lambo tools, scratch script runner           |
//! | `perms`     | M5        | permission gate at the tool-call boundary    |
//! | `vault`     | M6        | encrypted secret store, egress redaction     |
//! | `mcp`       | M10       | MCP client host for configured servers       |
//!
//! File-size discipline: soft target ~600 lines per file including tests and
//! doc text; CI fails past 1000. User-facing prose never counts against this —
//! it lives in `text/en.toml`. Split at seams into directory modules; keep
//! `mod.rs` thin (re-exports, shared types).

pub mod cli;
pub mod companion;
pub mod config;
pub mod home;
pub mod memory;
mod secure_path;
pub mod text;
pub mod tools;
pub mod vault;

/// Run one CLI invocation and return the process exit code:
/// `0` success · `2` user error · `1` internal failure (`cli::Failure`).
pub fn run() -> i32 {
    match cli::run() {
        Ok(()) => 0,
        Err(failure) => failure.report(),
    }
}
