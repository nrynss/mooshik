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
//!
//! **Escaped forms.** Scratch results are `serde_json`-serialized and recall
//! results are `serde_json::to_string`'d, so a value containing `"`, `\`, or
//! a control character (a PEM key's newlines) never appears literally in the
//! scanned string. Each token is therefore expanded to *both* forms — its
//! literal value and its JSON string-escaped form (exactly what
//! `serde_json` emits for the value as a JSON string member, surrounding
//! quotes stripped) — and the combined variant set is replaced longest-first,
//! so whichever encoding crossed the boundary is caught.
//!
//! **Arguments are deliberately not scanned.** Only results cross this
//! boundary; tool *arguments* flow outward before execution and are never
//! scanned. That is deliberate: the model cannot legitimately hold a value
//! (it only ever receives `[REDACTED]`), and scanning arguments would
//! false-positive on names, paths, and queries while protecting nothing.
//! Recorded here as a design decision, not an omission.

use serde_json::Value;
use std::sync::Arc;

use super::{ToolExecutor, ToolSpec};
use crate::vault::{lock_shared, SecretToken, SharedVault};

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

    /// Scan one finished tool result. Each token contributes its literal
    /// value *and* its JSON string-escaped form (scratch/recall results are
    /// serialized, so a value containing quotes, backslashes, or control
    /// characters crosses the boundary only in its escaped encoding). The
    /// combined variant set is replaced longest-first; empty values are
    /// skipped and an empty token set returns the output untouched.
    fn redact(&self, output: &str) -> String {
        let mut variants: Vec<String> = Vec::new();
        for token in self.tokens() {
            let value = token.expose();
            if value.is_empty() {
                continue;
            }
            variants.push(value.to_owned());
            let escaped = json_string_form(value);
            if escaped != value {
                variants.push(escaped);
            }
        }
        if variants.is_empty() {
            return output.to_owned();
        }
        // Longest-first over the union of forms: an overlapping prefix
        // redacts as the whole longer variant rather than leaving a mangled
        // partial replacement behind.
        variants.sort_by_key(|variant| std::cmp::Reverse(variant.len()));
        let mut safe = output.to_owned();
        for variant in &variants {
            safe = safe.replace(variant.as_str(), REDACTED);
        }
        safe
    }
}

/// The JSON string-escaped form of one secret value: exactly what
/// `serde_json` emits when the value is serialized as a JSON string member
/// (`\"`, `\\`, `\n`, `\t`, `\u00XX` for other control characters), with the
/// surrounding quotes stripped. `serde_json::to_string` on a `&str` cannot
/// fail and always wraps its output in quotes.
fn json_string_form(value: &str) -> String {
    let encoded = serde_json::to_string(value).expect("string serialization is infallible");
    encoded[1..encoded.len() - 1].to_owned()
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
    use serde_json::json;
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
    fn json_escaped_forms_of_secrets_are_redacted() {
        // P1-M6-1: a value containing a quote and a newline never appears
        // literally in a serialized result — only its JSON-escaped form does.
        // The redactor must catch that encoding too.
        const VALUE: &str = "line1\"quote\nline2";
        let vault = fixture_vault(&[("pem", VALUE)]);
        let serialized = json!({ "stdout": VALUE }).to_string();
        assert!(
            !serialized.contains(VALUE),
            "precondition: the literal form is absent from the encoded string"
        );
        let redactor = RedactingTools::new(Arc::new(Fixed(serialized)), Some(vault));
        let out = redactor.execute("tool", &Value::Null);
        assert_eq!(out, json!({ "stdout": REDACTED }).to_string(), "{out}");
    }

    #[test]
    fn escaped_variants_share_the_longest_first_order() {
        // A backslash-containing secret whose literal prefix overlaps another
        // secret: the union of literal + escaped forms must still resolve
        // longest-first deterministically.
        let vault = fixture_vault(&[("short", "sk-live"), ("long", "sk-liv\\eabc")]);
        let redactor = RedactingTools::new(
            Arc::new(Fixed(
                json!({ "out": "sk-liv\\eabcdef sk-live" }).to_string(),
            )),
            Some(vault),
        );
        let out = redactor.execute("tool", &Value::Null);
        // The escaped long secret (`sk-liv\\eabc` in the wire form) is longer
        // than `sk-live`, so it must win as one whole marker, not mangle.
        assert_eq!(
            out,
            json!({ "out": "[REDACTED]def [REDACTED]" }).to_string(),
            "{out}"
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
