//! What a failed invocation tells the terminal, and what it exits with.

use crate::{
    companion::CompanionError, config::ConfigError, home::HomeError, memory::MemoryError,
    vault::VaultError,
};

/// Why one CLI invocation failed, and therefore how the process should exit.
///
/// Exit-code convention (also documented in `--help`'s afterword):
///
/// * `0` — success.
/// * `2` ([`Failure::User`]) — the operator asked for something the current
///   setup cannot do: bad usage, invalid configuration, a name that does not
///   exist. Scripts may branch on this; the message says what to fix.
/// * `1` ([`Failure::Internal`]) — unexpected internal failure: broken state,
///   IO, or a bug. Retrying or reporting is the next step, not reconfiguring.
pub enum Failure {
    User(anyhow::Error),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        if is_user_error(&error) {
            Self::User(error)
        } else {
            Self::Internal(error)
        }
    }
}

impl Failure {
    /// The process exit code for this failure class.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::User(_) => 2,
            Self::Internal(_) => 1,
        }
    }

    /// The message the terminal sees: exactly the top-level `Display`, nothing
    /// else. Every error type renders what failed, why, and what to do next
    /// through `text/en.toml`, so the top level is complete by construction —
    /// while wrapped sources are NOT covered by that guarantee
    /// (`MemoryError::Backend` can wrap a store error whose detail names DSN
    /// material), so the chain never prints.
    pub(crate) fn rendered(&self) -> String {
        match self {
            Self::User(error) | Self::Internal(error) => error.to_string(),
        }
    }

    /// THE one place an error reaches the terminal. Print here or fix the code
    /// that bypasses this; do not grow a second formatter.
    pub fn report(&self) -> i32 {
        eprintln!("{}", self.rendered());
        self.exit_code()
    }
}

/// Whether the deepest known cause is something the operator authored and can
/// fix. Unknown error classes fail internal (exit 1): an unclassified error
/// must never punish a script with a misleading "you did it wrong".
fn is_user_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<ConfigError>().is_some()
            || cause.downcast_ref::<HomeError>().is_some_and(|error| {
                matches!(
                    error,
                    HomeError::MissingHome
                        | HomeError::UnsafePath
                        | HomeError::MigrationRequired
                        | HomeError::LayoutConflict
                )
            })
            || cause.downcast_ref::<VaultError>().is_some_and(|error| {
                matches!(
                    error,
                    VaultError::NotFound
                        | VaultError::InvalidName
                        | VaultError::MissingValue
                        | VaultError::NulByte
                        | VaultError::InputTooLarge
                        | VaultError::MissingPassphrase
                        | VaultError::Authentication
                        // The rest of the vault surface prints operator
                        // fix-it instructions ("restore a valid vault",
                        // "select passphrase mode"), so it is a refusal,
                        // not an internal failure.
                        | VaultError::InvalidFormat
                        | VaultError::UnsafePath
                        | VaultError::LockFailed
                        | VaultError::Keyring
                )
            })
            || cause.downcast_ref::<MemoryError>().is_some_and(|error| {
                matches!(
                    error,
                    MemoryError::MissingDsn | MemoryError::SessionConflict(_)
                )
            })
            || cause.downcast_ref::<CompanionError>().is_some_and(|error| {
                matches!(
                    error,
                    CompanionError::Unreachable
                        | CompanionError::Timeout
                        | CompanionError::HttpStatus
                        | CompanionError::TurnTooLarge
                        // "Check the endpoint" / "try again" are
                        // reconfiguration-style advice.
                        | CompanionError::InvalidResponse
                        | CompanionError::ToolLoop
                        // Credentials the operator supplies and fixes: a
                        // missing key file, a scope Google will not grant.
                        | CompanionError::AuthUnavailable
                        | CompanionError::AuthRefused
                )
            })
    })
}
