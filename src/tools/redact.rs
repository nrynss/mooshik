//! M6 — egress redaction: the ONE scan where a tool result crosses back to
//! the model (PLAN M6, SPEC *secrets are used, never shown*).
//!
//! [`RedactingTools`] wraps any [`ToolExecutor`]. Its [`ToolExecutor::execute`]
//! runs the inner executor, then scans the **final** result string against
//! every value currently in the vault ([`crate::vault::Vault::list`] then
//! [`crate::vault::Vault::get`] each) and replaces every occurrence with
//! `[REDACTED]`. Post-execute, pre-history: nothing reaches the model, the
//! transcript, or a later `lambo_derive` call unscanned — so a scratch script
//! that echoes `$TOKEN` is caught at the boundary, exactly once, for every
//! current and future tool.
//! Availability stance: chat does not live or die by the vault. A `None`
//! handle (locked vault, missing home) means *unredacted only because
//! unopenable* — a vault you cannot open cannot leak values either — and the
//! notice is printed once by the composition in [`super::executor_for_chat`].
//! An opened-but-empty vault makes redaction a cheap pass-through.
//!
//! Ordering within values: names are resolved longest-first so overlapping
//! prefixes redact as the whole longer secret rather than leaving a mangled
//! remainder.

use serde_json::Value;
use std::sync::Arc;

use super::{ToolExecutor, ToolSpec};
use crate::vault::{lock_shared, redact_output, SecretToken, SharedVault};

/// The `[REDACTED]` marker substituted for every secret occurrence.
pub const REDACTED: &str = "[REDACTED]";

/// A [`ToolExecutor`] that scans every inner result against the vault before
/// handing it back. Composes under the M5 permission gate:
/// `executor_for_chat` → [`super::GatedTools`] → this → tools.
pub struct RedactingTools {
    inner: Arc<dyn ToolExecutor>,
    vault: Option<SharedVault>,
}

impl std::fmt::Debug for RedactingTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Debug discipline: never render anything carrying plaintext. The
        // vault handle prints as its presence, not its contents.
        f.debug_struct("RedactingTools")
            .field("inner", &"ToolExecutor")
            .field("vault", &self.vault.is_some())
            .finish()
    }
}

impl RedactingTools {
    pub fn new(inner: Arc<dyn ToolExecutor>, vault: Option<SharedVault>) -> Self {
        Self { inner, vault }
    }

    /// Snapshot the current vault values, longest name first. Tokens are
    /// resolved fresh per call, so a rotated secret is picked up by the next
    /// tool result. The shared-vault lock is held only while resolving —
    /// never across the output scan.
    fn tokens(&self) -> Vec<SecretToken> {
        let Some(vault) = &self.vault else {
            return Vec::new();
        };
        let vault = lock_shared(vault);
        let mut tokens: Vec<SecretToken> = vault
            .list()
            .iter()
            .filter_map(|name| vault.get(name).ok())
            .collect();
        // Longest-first so an overlapping prefix redacts as the whole longer
        // secret rather than leaving a mangled partial replacement behind.
        tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
        tokens
    }

    /// Scan one finished tool result. Empty tokens are skipped inside
    /// [`redact_output`]; an empty token set returns the output untouched.
    fn redact(&self, output: &str) -> String {
        let tokens = self.tokens();
        if tokens.is_empty() {
            return output.to_owned();
        }
        redact_output(output, tokens)
    }
}

impl ToolExecutor for RedactingTools {
    fn specs(&self) -> Vec<ToolSpec> {
        self.inner.specs()
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        let output = self.inner.execute(name, arguments);
        self.redact(&output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::PassphraseProvider;
    use std::sync::Arc;

    /// An inner executor returning a fixed string.
    struct Fixed(String);

    impl ToolExecutor for Fixed {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn execute(&self, _: &str, _: &Value) -> String {
            self.0.clone()
        }
    }

    /// Open a throwaway vault preloaded with secrets; the caller drops the
    /// returned path guard after the vault handle.
    fn fixture_vault(secrets: &[(&str, &str)]) -> SharedVault {
        let dir = std::env::temp_dir().join(format!(
            "mooshik-redact-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut vault = crate::vault::Vault::open(
            dir.join("vault"),
            Arc::new(PassphraseProvider::new("pw").unwrap()),
        )
        .unwrap();
        for (name, value) in secrets {
            vault.set(name, value).unwrap();
        }
        vault.shared()
    }

    #[test]
    fn every_value_is_replaced_and_none_survives() {
        let vault = fixture_vault(&[("alpha", "one-value"), ("beta", "two-value")]);
        let redactor = RedactingTools::new(
            Arc::new(Fixed("one-value two-value clean".into())),
            Some(vault),
        );
        assert_eq!(
            redactor.execute("tool", &Value::Null),
            "[REDACTED] [REDACTED] clean"
        );
    }

    #[test]
    fn overlapping_prefixes_redact_longest_first() {
        let vault = fixture_vault(&[("short", "sk-live"), ("long", "sk-liveabc")]);
        let redactor = RedactingTools::new(
            Arc::new(Fixed("prefix sk-liveabcdef suffix sk-live".into())),
            Some(vault),
        );
        // Longest-first resolution: `sk-liveabcdef` becomes one marker plus
        // the leftover text, never a mangled partial replacement.
        assert_eq!(
            redactor.execute("tool", &Value::Null),
            "prefix [REDACTED]def suffix [REDACTED]"
        );
    }

    #[test]
    fn empty_tokens_are_skipped_without_corrupting_output() {
        let vault = fixture_vault(&[("blank", ""), ("real", "real-value")]);
        let redactor =
            RedactingTools::new(Arc::new(Fixed("value: real-value end".into())), Some(vault));
        assert_eq!(
            redactor.execute("tool", &Value::Null),
            "value: [REDACTED] end"
        );
    }

    #[test]
    fn zero_secrets_make_redaction_a_passthrough() {
        let vault = fixture_vault(&[]);
        let redactor = RedactingTools::new(Arc::new(Fixed("untouched".into())), Some(vault));
        assert_eq!(redactor.execute("tool", &Value::Null), "untouched");
    }

    #[test]
    fn no_vault_handle_passes_through_unredacted() {
        // The documented stance: without an openable vault there is nothing
        // to scan against; the executor must still answer.
        let redactor = RedactingTools::new(Arc::new(Fixed("plain".into())), None);
        assert_eq!(redactor.execute("tool", &Value::Null), "plain");
    }

    #[test]
    fn rotation_between_calls_is_observed() {
        let vault = fixture_vault(&[("rotating", "first-value")]);
        let redactor =
            RedactingTools::new(Arc::new(Fixed("first-value".into())), Some(vault.clone()));
        assert_eq!(redactor.execute("t", &Value::Null), REDACTED);
        lock_shared(&vault).set("rotating", "second-value").unwrap();
        // The inner executor now emits what used to be the secret's successor;
        // tokens are re-resolved per call, so the OLD value no longer matches
        // anything and the new one would be caught on a matching output.
        let rotated = RedactingTools::new(Arc::new(Fixed("second-value".into())), Some(vault));
        assert_eq!(rotated.execute("t", &Value::Null), REDACTED);
    }
}
