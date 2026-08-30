//! In-process Lambo memory: resolve backends, provision schema, open, serve —
//! and, in [`view`], read the open graph back as the workspace the TUI draws.

use lambo::LamboError;

use crate::text;

mod ops;
mod resolve;
pub mod view;

pub use ops::{open, provision, recall, serve, serve_plan, stats, ServePlan};
pub use resolve::{resolve_product, resolve_store};

/// Why workspace memory could not serve this invocation.
#[derive(Debug)]
pub enum MemoryError {
    MissingDsn,
    /// Lambo's single-writer lease is held by another process. The payload is
    /// Lambo's own conflict message — built from the session id, the holder
    /// token, and the lease age, never from DSN or credential material — and
    /// it carries the operator remediation (stop the other writer, or force
    /// a takeover), so it is rendered rather than flattened.
    SessionConflict(String),
    Backend(LamboError),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MissingDsn => text::get("memory.missing_dsn").to_owned(),
            // A backend failure of unknown shape: the wrapped cause may name
            // connection material, so only the fixed advice prints.
            Self::Backend(_) => text::get("memory.backend_failed").to_owned(),
            Self::SessionConflict(detail) => {
                text::get("memory.session_conflict").replace("{detail}", detail)
            }
        };
        f.write_str(&message)
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingDsn | Self::SessionConflict(_) => None,
            Self::Backend(error) => Some(error),
        }
    }
}

impl From<LamboError> for MemoryError {
    fn from(error: LamboError) -> Self {
        match error {
            // The conflict payload is operator-safe (session id + holder +
            // age) and actionable; everything else stays generic so no
            // store/embedder detail can reach the terminal.
            LamboError::Conflict(detail) => Self::SessionConflict(detail),
            other => Self::Backend(other),
        }
    }
}
