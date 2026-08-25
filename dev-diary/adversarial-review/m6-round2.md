# M6 adversarial review — round 2 (re-verification)

Scope: `4613aea` (`m6-vault-egress`, remediation of `m6-round1.md`). Method:
independent re-trace of both closures against the source (not the remediation
report's claims), an adversarial probe battery of seven transient tests driven
through the REAL composed stack (real bash scratch child, real vault, real
gate→redactor→tools ordering), then mutation re-testing of each closure.
All transient edits reverted; final gate on a clean tree at `4613aea`.

## Findings

**None at P1 or P2.** Both round-1 closures hold under independent trace,
adversarial probing, and mutation. One informational observation recorded
below (pre-existing since the M6 base commit, not a leak, not introduced by
the remediation).

### P1-M6-1 closure — VERIFIED

`RedactingTools::redact` (`src/tools/redact.rs`) now expands every token to its
literal value **and** `json_string_form(value)` — `serde_json::to_string` with
the surrounding quotes stripped, which is byte-exact what scratch
(`json!({...}).to_string()`) and recall (`serde_json::to_string(&recall)`,
`src/tools/mod.rs:229`) emit for `"`, `\`, `\n`, `\t`, `\b`, `\f`, `\r`, and
`\u00XX` control bytes. Non-ASCII (é) is emitted verbatim by serde_json, so it
is already covered by the literal form; confirmed by probe. Empty tokens are
skipped before any push; the union of literal + escaped forms replaces
longest-first.

Adversarial probes through the real composed stack (fixture vault → gate →
redactor → real `MemoryTools` scratch running bash with the secret injected via
env):

- `q"ote\slash<TAB>tab` (quote + backslash + tab): literal absent, escaped wire
  form absent, `"stdout":"[REDACTED]"`.
- `c\u{1}af\u{7}é` (control bytes + non-ASCII): same — the `\u0001`/`\u0007`
  encodings die at the boundary.
- PEM-shaped multi-line value: fully redacted in the serialized result.
- All three hostile values echoed in ONE output: three independent markers, no
  fragment of any encoding survives.

The serialized result reaching history contains no recoverable form. History
ingestion itself is unchanged: `Session::turn` pushes exactly what
`executor.execute` returns (`src/companion/session.rs:121`), and that return
value is post-redaction.

### P2-M6-2 closure — VERIFIED

`compose_chat_stack` (`src/tools/mod.rs:479-486`) is the single composition
seam: redactor wraps inner, gate wraps redactor. The behavioral pin
`the_production_composition_redacts_secrets_behaviorally` calls
`super::compose_chat_stack` directly — the SAME function body
`executor_for_chat` delegates to at `mod.rs:470` (not a parallel test-only
stack; the only difference is the injected inner executor, which is exactly the
seam's parameter). The structural pin binds the factory to route through the
seam (`factory.contains("compose_chat_stack(inner")`) and binds the seam body
to `RedactingTools::new(inner)` inside `Arc::new(GatedTools::new(redacting…))`.

Bypass probe: grep over all non-test construction sites — `RedactingTools::new`
and `GatedTools::new` appear in production code ONLY inside
`compose_chat_stack`. `cli.rs:168` builds chat solely via
`executor_for_chat`; there is no second factory, no way to assemble the chat
stack around the seam. (Test files hand-compose stacks, which is expected.)

### Informational — marker self-mangling when a secret's variant is a substring of `[REDACTED]` (no leak)

Sequential longest-first replace can mangle previously inserted markers if some
secret's variant is a suffix of / straddles `[REDACTED]`. Empirically:

- secret `ED]` alongside a real secret → `x [REDACT[REDACTED] y`
- secret `]x` after a marker abutting text → `[REDACTED[REDACTED] tail`

In both cases **zero plaintext reaches the output** — replacements only ever
insert markers, so corruption degrades the cosmetic marker, never reveals the
redacted value. This is inherent to literal-replace redaction, existed at the
M6 base commit (`071c3cc` lineage), is not touched by the remediation, and the
round-1 note ("a value equal to `[REDACTED]` degenerates harmlessly")
understated the class slightly. Not residue; recording so it is a known,
deliberate limit like the P3-M6-3 cap-split.

## New-residue hunt (all clean)

- **Empty token**: skipped pre-push (`value.is_empty()` continue); pinned by
  `empty_tokens_are_skipped_without_corrupting_output`. Probe confirms no
  corruption from empty variants.
- **Pure-control-char token** (`\u{1}\u{2}\u{1f}`): wire form `\u0001…`
  differs from literal, gets pushed as a variant, and is caught in a
  serialized result (probe: stdout member fully `[REDACTED]`).
- **Very long tokens / perf**: 200 secrets (~100 B each) scanned against a
  64 KB output completes well under the 500 ms probe bound — O(variants ×
  output) replace per tool call is fine at realistic vault sizes; tokens are
  snapshotted once per call, not per variant.
- **Double-redaction / nested forms**: short secret's literal inside long
  secret's escaped form resolves cleanly via longest-first over the union —
  probe yields one whole `[REDACTED]`, no residue, no `[REDACTED]CTED]`
  mangling for prefix/nested overlaps. Only the marker-substring case above can
  mangle (cosmetically).
- **Rotation pins**: `rotation_between_calls_is_observed` +
  `injection_resolves_per_run_so_rotation_is_observed` pass in the suite;
  tokens re-resolve per call through `tokens()`'s fresh lock.
- **SPEC example loads / M4/M5 intact**: full suite green (178) including the
  config-matrix pins, gate deny-string pins, prompt-once
  (`always_confirmed` under the gate), and chat-without-memory Noop fallback
  pins.
- **File caps unchanged**: `SCRATCH_MAX_OUTPUT_BYTES` = 64 KiB with the new
  not-secret-aware cut documented at `ScratchConfig::max_output_bytes`
  (`src/tools/scratch.rs:99-103`).
- **en.toml completeness**: the remediation commit touches exactly two keys —
  adds `vault.nul_byte` and rewords `tools.vault_unavailable`; both present,
  both referenced (`VaultError::Display` mapping and the factory notice), and
  the degradation wording names `MOOSHIK_VAULT_PASSPHRASE` explicitly (pinned
  by `the_vault_unavailable_notice_names_the_silent_passphrase_degradation`).
- **P3 spot-checks**: `Vault::set` rejects interior NUL → `VaultError::NulByte`
  (`vault.rs:313-318`) with en.toml message and the
  `nul_byte_value_is_rejected_at_set` pin; args-unscanned decision recorded in
  the `redact.rs` module doc as a deliberate boundary decision; truncation
  limit documented at `max_output_bytes`.

## Mutation table

| # | Mutation | Pins exercised | Result |
|---|----------|----------------|--------|
| A | Remove the escaped-form push (`let escaped = …; if escaped != value { variants.push(escaped) }`) from `RedactingTools::redact` | `json_escaped_forms_of_secrets_are_redacted`, `json_escaped_multiline_secret_is_redacted_in_the_serialized_result` | BOTH FAILED — probe output showed `line1\"quote\nline2` surviving verbatim (**CAUGHT**); also took down `escaped_variants_share_the_longest_first_order`. Restored; tree verified clean. |
| B | Remove `RedactingTools` from `compose_chat_stack` (gate directly on inner) | `the_production_composition_redacts_secrets_behaviorally`, `executor_for_chat_composes_gate_then_redaction_then_tools` | BOTH FAILED — behavioral assertion got the unredacted echo (`leak: factory-boundary-secret`); structural pin rejected the wrap-less seam body (**CAUGHT**). Restored; tree verified clean. |

## Gate table

| Gate | Command | Result |
|------|---------|--------|
| Baseline suite | `cargo test --locked` | 178 passed, 0 failed, 1 ignored |
| Format | `cargo fmt --all -- --check` | PASS |
| Lint | `cargo clippy --all-targets --locked -- -D warnings` | PASS |
| Post-review gate (clean tree) | `cargo test --locked` | 178 passed, 0 failed, 1 ignored |
| Tree state after review | `git status --short` | clean except this document |

## Verdict

**APPROVE — zero residue.** Both round-1 closures are real and pinned by
mutations that fail loudly; the adversarial battery found no path by which any
encoding of a vault value crosses the serialized scratch/recall boundary to
history. The one new observation (marker self-mangling on marker-substring
secrets) leaks nothing, predates the remediation, and belongs beside the
documented cap-split as a known limit.
