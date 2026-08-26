# M7 remediation — round 1

Branch `m7-cli-sweep`, base `4dbf798`. Responds to `m7-round1.md`. Scope:
P1-a, P2-b/c/d/e fixed; P3-g/P3-h (cheap) fixed; P3-f/P3-i/P3-j documented,
not changed. All gates green at commit time; tree clean.

## Per-finding fixes

### P1-a — empty secret value exits 1 → **fixed**

`normalize_environment_value` / `normalize_stdin_bytes` now raise typed
variants (`VaultError::MissingValue`, `VaultError::InputTooLarge`,
`VaultError::Io`) instead of bare `anyhow!(text::get(...))`, so the chain
carries a known class and `is_user_error` stays the single decision point.
Messages are unchanged (the variants render the same en.toml strings).

Pins: `empty_secret_values_classify_user_with_the_missing_value_message`
(both paths: message verbatim, typed variant in the chain, exit 2) and
`oversized_unreadable_and_non_utf8_secret_input_is_typed_too`. Live probe:
env-empty and stdin-empty both print `vault.missing_value` verbatim and exit
2.

### P2-b — unpinned print inside `Failure::report` → **fixed**

New integration test `tests/report_pin.rs` drives the real binary as a
subprocess against a fixture home whose config points Postgres at a refused
local port with planted DSN material, then asserts stderr byte-for-byte:
exactly `memory.backend_failed\n`, exit 1, no DSN substring, empty stdout.

Mutation-verified: re-introducing the reviewer's M2 mutation faithfully
(`match self { Self::User(error) | Self::Internal(error) => eprintln!("{error:#}") }`
inside `report`) fails the pin with observed stderr
`…credentials.: backend: pool timed out while waiting for an open connection`.
**CAUGHT.** Restored; pin green again. (A naive `{:#}` on `self.rendered()`
is vacuous — it formats a String — which is exactly why the mutation was
applied to the error itself.)

### P2-c — lease conflicts flatten actionable detail → **fixed**

* `MemoryError` gains `SessionConflict(String)`. The single
  `From<LamboError>` impl routes `LamboError::Conflict(detail)` there;
  everything else stays `Backend(_)`. Lambo builds the conflict payload from
  session id + holder + lease age only (no DSN/credential material), so the
  safe detail is routed through the new `memory.session_conflict` key
  (`"Workspace memory is held by another writer, so this command was
  refused: {detail}"`), which carries lambo's remediation (stop the other
  writer / force a takeover).
* Classified user error (exit 2): `MemoryError::SessionConflict(_)` joined
  into `is_user_error`. Generic `Backend` failures still render the fixed
  message and stay internal.
* Pins: `lease_conflicts_classify_user_and_render_holder_remediation`
  (exit 2, situation + takeover hint present, no DSN) and
  `lambo_conflicts_map_to_the_session_conflict_variant_not_the_generic_backend`.

Honesty half (executor close skipped on the failure path):
`companion::run_chat` now binds the `block_on` outcome first, drops the
executor unconditionally, then returns the outcome — so chat's graceful
memory close runs before any classified failure propagates, preserving M4's
sync-frame-drop contract. Behavioral injection of a chat-loop failure is not
reachable in fixtures (the loop's only error exits are runtime-build and
stdin read), so the pin is source-structural:
`run_chat_closes_the_executor_on_the_failure_path_too` asserts the block_on
line carries no `?` and that `drop(executor)` follows it unconditionally.

Note: a live recall-during-held-lease probe needs the real Postgres lease
path (fixture memory stores are per-handle), so classification is pinned at
the variant level rather than end-to-end.

### P2-d — operator-fixable variants exiting 1 → **fixed** (classes widened)

Per the afterword convention ("2 = refused … configuration"), the User set
now includes `HomeError::{UnsafePath, MigrationRequired, LayoutConflict}`,
`VaultError::{InvalidFormat, UnsafePath, LockFailed, Keyring}`,
`CompanionError::{InvalidResponse, ToolLoop}` — every one of their en.toml
messages gives reconfiguration-style instructions. `exit_codes_distinguish_user_error_from_internal_failure`
covers each named variant (and `VaultError::Keyring` moved out of the
internal list).

### P2-e — raw detail at the tool boundary → **fixed**

* `lambo_err`: raw `{error}` Display dropped (a `Store` wrap can name DSN
  hosts); prints/returns the fixed `tools.memory_tool_failed` notice.
* Panic site: raw payload dropped entirely (payloads are arbitrary data and
  may carry vault values); prints the fixed `tools.tool_panicked` notice.
  The `panic_message` helper is deleted.
* Pin `tool_boundary_stderr_notices_route_through_en_toml_without_raw_detail`
  (structural: both sites route through `text::get`, no raw format strings or
  helper remain; notices carry no placeholders) plus behavioral
  `lambo_err_returns_the_fixed_notice_not_the_lambo_display` (planted DSN
  material never reaches the result string).

Mutation-verified: restoring either raw print fails the pins. **CAUGHT**
(both sites individually).

## Cheap P3s taken

* **P3-g**: `tools.close_failed` now ends "Recent changes may not have been
  saved; try the command again."
* **P3-h**: `validate_name` rejects leading `-` (arg-safety for
  `secret list | xargs mooshik secret get`); `vault.invalid_name` says so.
  Pinned in `names_and_missing_values_are_distinct_and_safe`. Note clap also
  refuses `-flaglike` at parse time (exit 2); the validator covers `--`
  -escaped names and config-time validation.

## Documented, not changed

* **P3-f** (parser tokenizes raw TOML): unescaping TOML basic strings before
  tokenizing is a real change to the extraction contract; left for its own
  round rather than risk a silent divergence mid-remediation.
* **P3-j** (diary omissions): this file is the record; diary edits deferred
  to the integrator's docs pass.

## Mutation log (this round)

| Mutation | Pin expected to catch | Result |
| --- | --- | --- |
| Cause-chain print (`{error:#}`) inside `Failure::report` | `tests/report_pin.rs::report_prints_only…` | **CAUGHT** |
| Raw `LamboError` Display restored in `tools::lambo_err` | `tool_boundary_stderr…`, `lambo_err_returns_the_fixed_notice…` | **CAUGHT** (both) |
| Raw panic-payload print restored in `ToolExecutor::execute` | `tool_boundary_stderr…` | **CAUGHT** |
| Untype env empty-value back to bare `anyhow!` | `empty_secret_values_classify_user…` | **CAUGHT** |

All mutations reverted; final tree diff contains none of them.

## Gates (tree clean except this file at commit time)

```
cargo fmt --all -- --check                              clean
cargo clippy --all-targets --locked -- -D warnings      clean
cargo test --locked                                     195 passed · 0 failed · 1 ignored
```

Live probes after the fix: empty env/stdin value → `vault.missing_value`
verbatim, exit 2 (was 1); `secret set -- -flaglike` → invalid-name message,
exit 2.
