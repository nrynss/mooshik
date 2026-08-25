# M6 remediation — round 1 findings

**Date**: 2026-08-26
**Base**: `013c3cc` (`m6-vault-egress`), findings from `m6-round1.md`.
**Scope**: P1-M6-1 and P2-M6-2 fixed; actionable P3s (P3-M6-4/5/6 plus the
P3-M6-3 documentation) fixed; no half-fixes for genuinely-inherent limits —
P3-M6-3's cap-split is documented as a deliberate limit, not changed.

## Per-finding changes

### P1-M6-1 — JSON escaping defeats redaction → FIXED
`RedactingTools::redact` (`src/tools/redact.rs`) now expands every token to
*both* forms before scanning: its literal value **and** its JSON string-escaped
form (`json_string_form` — `serde_json::to_string` on the value with the
surrounding quotes stripped, i.e. exactly what scratch/recall serialization
emits for `\"`, `\\`, `\n`, `\t`, and `\u00XX` control bytes). The combined
variant set is replaced longest-first, so whichever encoding crossed the
boundary is caught deterministically (the overlap pin's invariant extends to
the union). The module doc records why: PEM keys are newline-separated, and
any value containing a quote/backslash/control char never appears literally in
a serialized result.

New pins:
* `json_escaped_multiline_secret_is_redacted_in_the_serialized_result`
  (`src/tools/tests.rs`) — the reviewer's probe verbatim: a script echoes a
  secret containing a double quote and a newline through the composed stack;
  the result must carry `"stdout":"[REDACTED]"` and no literal or escaped
  fragment (`line1`, `quote`, `line2`, `\"quote`, `\nline2`) may survive.
* `json_escaped_forms_of_secrets_are_redacted` (`src/tools/redact.rs` unit) —
  serialized-JSON input redacted to the `[REDACTED]` wire form.
* `escaped_variants_share_the_longest_first_order` (`src/tools/redact.rs`) —
  longest-first resolution across the union of literal + escaped forms.

**Mutation result**: removing the escaped-form push (`if escaped != value {
variants.push(escaped) }`) from `RedactingTools::redact` fails BOTH named pins
(`json_escaped_forms_of_secrets_are_redacted`,
`json_escaped_multiline_secret_is_redacted_in_the_serialized_result`) —
**CAUGHT**, restored.

### P2-M6-2 — factory composition unbound behaviorally → FIXED
The gate→redaction→tools composition is extracted from `executor_for_chat`
into a named seam, `compose_chat_stack(inner: Arc<dyn ToolExecutor>, vault:
Option<SharedVault>, grants: Grants) -> Arc<dyn ToolExecutor>`
(`src/tools/mod.rs`). The production factory delegates to it unchanged, and a
new behavioral test drives the REAL composed stack over a fixture vault:
`the_production_composition_redacts_secrets_behaviorally`
(`src/tools/tests.rs`) composes an echo-ish inner executor holding a secret,
calls through the returned executor, and asserts the answer is
`"leak: [REDACTED]"`. Dropping `RedactingTools` now fails behaviorally, not
just textually. The structural pin
(`executor_for_chat_composes_gate_then_redaction_then_tools`) is kept and
updated: the factory must build via `compose_chat_stack`, and the seam body
must still wrap `RedactingTools::new(inner)` inside
`Arc::new(GatedTools::new(redacting…))`.

**Mutation result**: removing `RedactingTools` from `compose_chat_stack`
fails BOTH `the_production_composition_redacts_secrets_behaviorally` (asserted
`leak: [REDACTED]`, got the unredacted echo) and the structural pin —
**CAUGHT**, restored.

## Cheap / documented P3s

| ID | Change |
| --- | --- |
| P3-M6-3 | Documented as a deliberate limit, not changed: `ScratchConfig::max_output_bytes` (`src/tools/scratch.rs`) now states the cap cut is not secret-aware, so a cap landing inside a secret occurrence leaves the surviving prefix unmatchable/unredacted — accepted because the script author already holds the plaintext (accidental-echo defense, not a new grant). |
| P3-M6-4 | `Vault::set` rejects interior NUL (`VaultError::NulByte`, en.toml `vault.nul_byte`) — a stored NUL cannot survive `Command::env`, so it would silently break every injected run. Pin: `nul_byte_value_is_rejected_at_set` (`src/vault.rs`), mirroring the oversized-value parse-style test. |
| P3-M6-5 | The args-unscanned decision is recorded at the boundary (`src/tools/redact.rs` module doc: arguments flow outward pre-execution, scanning would false-positive on names/paths/queries while protecting nothing) and in this record. |
| P3-M6-6 | `tools.vault_unavailable` wording now states that `provider = "passphrase"` with `MOOSHIK_VAULT_PASSPHRASE` unset degrades silently to this state and that degraded mode is not protection. Pin: `the_vault_unavailable_notice_names_the_silent_passphrase_degradation` (`src/tools/tests.rs`). |

Suite grew 172 → 178 (+6 tests).

## Mutation summary

| # | Mutation | Result |
| --- | --- | --- |
| 1 | Drop escaped-form replacement from `RedactingTools::redact` | **CAUGHT** — both escaped-form pins fail |
| 2 | Remove `RedactingTools` from `compose_chat_stack` | **CAUGHT** — behavioral factory test fails on unredacted echo + structural pin fails |

## Gates

```
cargo fmt --all -- --check                                  PASS
cargo clippy --all-targets --locked -- -D warnings          PASS
cargo test --locked                                         PASS — 178 passed, 0 failed, 1 ignored
```

Tree clean after commit; review file left as-is.
