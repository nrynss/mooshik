# M6 adversarial review — round 1

Scope: `013c3cc` (m6-vault-egress). Vault injection into scratch env + egress
redaction at the tool-result boundary. Method: full read of the M6 diff against
`6ff8f4a`, attack pass over completeness / plaintext escape / injection /
availability / config / pins / regression, then mutation testing of the five
key pins, then two transient probes (config matrix, escaped-secret bypass).
All transient edits reverted; final gate on a clean tree.

## Findings

### P1-M6-1 — JSON escaping defeats redaction for any value containing `"`, `\`, or control characters

`RedactingTools::redact` scans the **final result string**, which for scratch
is `serde_json`-serialized (`json!({...}).to_string()`) and for recall is
`serde_json::to_string(&recall)`. `redact_output` replaces **literal**
substrings. A vault value that serializes with escapes — quotes, backslashes,
newlines, tabs, any byte < 0x20 — never appears literally in the encoded
string, so the scan misses it entirely.

Empirically confirmed through the composed stack (gate → redactor → tools)
with secret `line1"quote\nline2`:

```
PROBE_OUT: {"duration_ms":20,"exit_code":0,"stderr":"","stdout":"line1\"quote\nline2",...}
```

The value crossed the boundary to the model/history unredacted. The same gap
applies to `lambo_recall` results whose concept content embeds such a value.
Multiline secrets are not exotic: PEM private keys are newline-separated.

Fix direction: redact per-field *before* serialization (scratch stdout/stderr,
recall render), or scan both the raw and the JSON-escaped form of each token.
Add a regression pin for a multiline/quoting secret.

### P2-M6-2 — No behavioral test binds `executor_for_chat`'s redaction wrap

Mutation M1 (removing `RedactingTools` from the composition) was caught only
by the textual source pin
(`executor_for_chat_composes_gate_then_redaction_then_tools`). The echo
round-trip, multi-secret, and transcript-hygiene pins all kept passing because
they hand-compose their own stacks (`composed_stack`,
`RedactingTools::new(Arc::new(Echo), …)`) and never route through the factory.
The structural pin matches the established M3/M5 convention and does bind the
order, but the security boundary currently has zero behavioral end-to-end
coverage of the *production* composition. Recommend either an injectable
memory seam for `executor_for_chat` tests or an explicit decision note that
the composition is pinned textually by design.

### P3-M6-3 — Output-cap truncation can split a secret so its prefix escapes

`read_capped` cuts captured bytes at an arbitrary boundary. If the cut lands
inside a secret occurrence, the surviving prefix does not match the token and
escapes redaction (e.g. cap lands mid-`sk-liveabcdef`). The script author
already holds the plaintext, so this is a partial-leak nuance of the accidental-
echo defense rather than a new grant; note it in the boundary docs or make the
cap secret-aware (e.g. scan before truncation is impossible post-hoc — instead
scan the raw buffer before lossy truncation).

### P3-M6-4 — NUL inside a secret value breaks every injected run

Vault values may contain interior NUL (`secret set` via stdin accepts any
UTF-8). Verified outside the repo: `Command::env` fails spawn with
`nul byte found in provided data` → mapped to the contained
`tools.scratch_spawn_failed` error. No crash, no leak, but the configured
injection silently becomes unusable for that secret. Consider rejecting NUL at
`Vault::set` like `validate_scratch` rejects it for code.

### P3-M6-5 — "Arguments are not scanned" is practiced but undocumented

Only results cross the redactor; tool arguments never do. That is the right
call (the model cannot legitimately hold a value, and scanning args would
false-positive), but neither the module docs nor the implementation diary
states it as a deliberate decision. Record it.

### P3-M6-6 — No interactive passphrase fallback on the chat path

`provider_for` reads `MOOSHIK_VAULT_PASSPHRASE` only. With
`provider = "passphrase"` and the env var unset, chat degrades to the one
notice + unredacted mode; there is no prompt. Consistent with the documented
unattended-start stance, but worth a line in the notice/docs so users do not
mistake degraded mode for protection.

## Verified non-findings (attack list)

- **Completeness:** `Session::turn`'s `self.executor.execute(...)` is the only
  model-facing tool path and receives the composed stack from
  `executor_for_chat`; `session.rs` has zero diff. Denial strings are produced
  by the gate *before* execution (nothing to scan); malformed args produce
  static text. The scan sees the post-truncation, post-denial final string
  (modulo P3-M6-3). Recall/derive/stats results are scanned too, so pre-M6
  graph content still in the vault is caught on egress.
- **Plaintext escape:** `expose()` call sites are exactly `Command::env` in
  `interpreter_command`, the by-design `secret get` CLI print, and tests;
  `config show` maps the API key to `***REDACTED***`. `SecretToken`
  Debug/Display print `[REDACTED]`; it is neither `Clone` nor constructible
  outside `vault.rs` (mutation attempts failed to compile — good property).
  Locks are leaf-scoped around `get`/`list`/`set`, never across awaits
  (`execute` is sync) or spawns; `lock_shared` recovers poison.
- **Injection correctness:** all-or-nothing resolution completes before spawn;
  rotation re-reads per run (both pinned); env-var names fail closed at load
  to `[A-Za-z_][A-Za-z0-9_]*`; names ≤64 chars restricted charset; errors name
  at most the secret name. Longest-first ordering makes overlapping prefixes
  deterministic (`sk-live` vs `sk-liveabc` pinned); sequential literal replace
  is deterministic; a value equal to `[REDACTED]` degenerates harmlessly.
- **Availability:** the vault-unavailable notice prints once at composition,
  not per call; empty vault = one cheap lock+empty-list per call; non-empty
  env table without a vault aborts before spawn (`scratch_env_unavailable`),
  no half-started child possible.
- **Config fail-closed matrix** (probe): nested table under
  `[tools.scratch.env]`, empty key, empty name, duplicate keys, unknown keys
  in `[tools]`/`[tools.scratch]` all fail the load; the SPEC `[permissions]`
  example loads alongside an env table; `config show` renders names only.
- **Regression:** M4/M5 pins intact (only mechanical `for_chat(config, …)` /
  `secret_env` field updates); gate deny strings unchanged; scratch
  prompt-once preserved (`always_confirmed` under the gate); chat-without-
  memory Noop fallback still gated and answering; live-test untouched (not in
  the diff).

## Mutation table

| # | Mutation | Pin expected to catch | Result |
|---|----------|----------------------|--------|
| M1 | Remove `RedactingTools` from `executor_for_chat` | echo round trip + transcript pin | **Only the structural source pin FAILED** (`executor_for_chat_composes_gate_then_redaction_then_tools`); behavioral pins passed — they hand-compose their stacks (→ P2-M6-2) |
| M2 | `resolve_injection` skips missing secrets instead of aborting | missing-secret abort | `missing_secret_fails_the_script_before_it_starts` FAILED ✓ |
| M3 | Sort tokens ascending (break longest-first) | overlap test | `overlapping_prefixes_redact_longest_first` FAILED ✓ |
| M4 | Vault-unavailable short-circuits to bare `NoopExecutor` | availability test | `executor_for_chat_gates_even_the_noop_fallback` FAILED ✓ |
| M5 | Freeze first-read values in `Vault::get` (cache values) | rotation pins | `rotation_between_calls_is_observed` + `injection_resolves_per_run_so_rotation_is_observed` both FAILED ✓ |

Note: naive caching mutations at the `RedactingTools` layer do not compile —
`SecretToken` is neither `Clone` nor externally constructible. Encapsulation
held up under attack.

## Gate table

| Gate | Command | Result |
|------|---------|--------|
| Baseline suite | `cargo test` | 172 passed, 0 failed, 1 ignored |
| Post-review gate (clean tree) | `cargo test` | 172 passed, 0 failed, 1 ignored |
| Tree state after review | `git status --short` | clean except this document |

## Verdict

**NEEDS WORK (one round).** The architecture is right — single boundary,
correct composition order, honest availability split, tight plaintext scope,
and four of five mutation pins catch real regressions. But P1-M6-1 means the
headline guarantee ("every tool result scanned against ALL vault values") does
not hold for any value containing a quote, backslash, or newline — PEM keys
and multiline tokens sail straight through to the model and the graph. Fix the
escape-aware scan (or per-field pre-serialization redaction), add the
regression pin, and resolve P2-M6-2 with either a behavioral factory test or a
documented reliance on the structural pin.
