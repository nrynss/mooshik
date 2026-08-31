//! In-process Lambo memory: resolve backends, provision schema, open, serve —
//! and, in [`view`], read the open graph back as the workspace the TUI draws.

use lambo::LamboError;

use crate::text;

mod ops;
mod reflect;
mod resolve;
pub mod view;

pub use ops::{open, provision, recall, serve, serve_plan, stats, ServePlan, WriteLane};
pub use reflect::{
    plan_reflect, prose_for_day, read_prose_for_view, reason_for_thread, run_reflect, DayProse,
    FixtureReflector, ProseConcept, ProseIndex, ReflectError, ReflectOutcome, Reflector,
    Target as ProseTarget,
};
pub use resolve::{resolve_product, resolve_store};

/// Why workspace memory could not serve this invocation.
#[derive(Debug)]
pub enum MemoryError {
    MissingDsn,
    /// Lambo's single-writer lease is held by another process. The payload is
    /// Lambo's own conflict message — built from the session id, the holder
    /// token, and the lease age, never from DSN or credential material — so the
    /// facts in it are rendered rather than flattened. The remediation is
    /// Mooshik's; see [`facts`].
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
                text::get("memory.session_conflict").replace("{detail}", facts(detail))
            }
        };
        f.write_str(&message)
    }
}

/// The facts of Lambo's refusal, without the remediation it appends to them.
///
/// Lambo's first sentence names the session, the holder and the lease age, and
/// all of it is operator-safe. What follows it is advice that is true in Lambo's
/// tree and false here: it offers a forced takeover, which this binary exposes
/// no way to perform, and points at `docs/reference/cli.mdx`, which is a file in
/// Lambo's repository and in no release of Mooshik — `mooshik/docs` holds one
/// document. An operator sent to a page that does not exist is worse off than
/// one sent nowhere, so the advice is Mooshik's own and lives in
/// `memory.session_conflict`, where it can be kept true.
///
/// The cut is the first sentence boundary, which is where Lambo's own facts
/// stop. A message with no advice appended is passed through whole.
fn facts(detail: &str) -> &str {
    match detail.find(". ") {
        Some(end) => &detail[..=end],
        None => detail,
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

impl From<ReflectError> for MemoryError {
    fn from(error: ReflectError) -> Self {
        Self::Backend(match error {
            ReflectError::Backend(error) => error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lambo's own refusal, verbatim, as `Memory::builder` returns it.
    const HELD: &str = "session mooshik is already held by another writer \
        (mooshik@laptop#52769) — it acquired the single-writer lease 3s ago and is still \
        refreshing it. Refusing to open a second writer. If that holder is wedged, an \
        operator can force a takeover (see the single-writer lease note in \
        docs/reference/cli.mdx)";

    /// The refusal keeps every fact and sends the operator somewhere that
    /// exists.
    #[test]
    fn a_session_conflict_names_the_holder_and_no_page_this_product_does_not_ship() {
        let rendered = MemoryError::SessionConflict(HELD.to_owned()).to_string();
        assert!(rendered.contains("mooshik@laptop#52769"), "{rendered}");
        assert!(rendered.contains("3s ago"), "{rendered}");
        assert!(rendered.contains("still refreshing it"), "{rendered}");
        assert!(!rendered.contains(".mdx"), "{rendered}");
        assert!(!rendered.contains("takeover"), "{rendered}");
        // And says what to do about it in this product's own words.
        assert!(rendered.contains("mooshik serve"), "{rendered}");
    }

    /// A refusal that appends no advice is passed through whole, so the cut can
    /// never take a fact with it.
    #[test]
    fn a_conflict_with_nothing_appended_keeps_all_of_it() {
        let bare = "session mooshik is already held by another writer (host#1)";
        assert_eq!(facts(bare), bare);
        assert!(MemoryError::SessionConflict(bare.to_owned())
            .to_string()
            .contains(bare));
    }
}
