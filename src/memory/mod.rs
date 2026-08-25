//! In-process Lambo memory: resolve backends, provision schema, open, serve.

use lambo::LamboError;

use crate::text;

mod ops;
mod resolve;

pub use ops::{open, provision, serve, serve_plan, ServePlan};
pub use resolve::{resolve_product, resolve_store};

#[derive(Debug)]
pub enum MemoryError {
    MissingDsn,
    Backend(LamboError),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
            Self::MissingDsn => "memory.missing_dsn",
            Self::Backend(_) => "memory.backend_failed",
        };
        f.write_str(text::get(key))
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingDsn => None,
            Self::Backend(error) => Some(error),
        }
    }
}

impl From<LamboError> for MemoryError {
    fn from(error: LamboError) -> Self {
        Self::Backend(error)
    }
}
