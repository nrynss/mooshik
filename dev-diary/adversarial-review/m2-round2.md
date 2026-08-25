# Adversarial review — Mooshik M2/M2b, round 2

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: commit `33a5bcb` on top of `5ed3e68`, round-1 findings, remediation record. Lambo `/home/nryn/work/lambo` @ `f90a662`.
**Worktree**: `/home/nryn/.grok/worktrees/work-mooshik/subagent-01a037b8-8dfd-7ea1-8816-f2b12b352e5e` @ 33a5bcb
**Verdict**: **APPROVE** — 0 P1 / 0 P2 / 0 P3

## Method

Read round 1 (`dev-diary/adversarial-review/m2-round1.md`) and `m2-remediation-round1.md`. Re-traced each claimed closure in current source: `src/config/{mod,overlay,show}.rs`, `src/memory/{mod,resolve,ops}.rs`, `src/home.rs`, `src/cli.rs`, `src/text/en.toml`, `Cargo.toml` / `Cargo.lock`. Traced Lambo @ `f90a662` for public `store_dsn_identity` (`src/store/dsn.rs:89`, crate-root re-export `src/lib.rs:129`), `SessionEndpoint::for_store` / `store_is_shareable`, `ServeOptions::new` (stdio), `mcp::serve` endpoint publish, `build_store_with_vector_dim`.

Mutation-tested every new pin named in round 1's suggestions (break the fix, run the named test, restore). Extra hunt: stub `serve()` to `Ok(())` against the full lib suite; drop `deny_unknown_fields` on `StoreSection`. Transient edits restored; source tree matches HEAD except this file.

## Round-1 closure table

| ID | Verdict | Independent evidence |
| --- | --- | --- |
| P2-M2-1 | **HOLDS** | `overlay_dsn` compares `lambo::store_dsn_identity` (`src/config/overlay.rs:144–167`), not trimmed strings. Env spelling wins when identities match. Mutations: identity → `trim()` equality fails `password_overlay_of_one_database_is_accepted` and `omitted_postgres_port_is_the_same_database`; `same_database` always-true fails `different_hosts_are_a_dsn_conflict_without_echoing_secrets`; putting `s3cret`/`hunter2` in `config.dsn_conflict` fails that test. `config show` redaction still asserted in the password-overlay test. |
| P2-M2-2 | **HOLDS** | `StoreSection` / `EmbedderSection` (`src/config/mod.rs:156–233`) carry product field defaults (`kind = postgres`, `dim = 1536`, Gemini location/model) and `deny_unknown_fields`. Converted via `to_lambo()`. Mutations: `default_store_kind` → Memory fails empty `[store]` and DSN-only `[store]`; `DEFAULT_EMBED_DIM = 1024` fails `gemini_table_without_dim_uses_product_default`. Dropping `deny_unknown_fields` on `StoreSection` fails `unknown_and_malformed_values_are_rejected`. |
| P2-M2-3 | **HOLDS** | `provision(&Config::default())` is `MissingDsn` (`src/memory/ops.rs:151–156`); stubbing `provision` to `Ok(())` fails that test. `serve_plan` (`src/memory/ops.rs:46–56`) pins session/agent, `Transport::Stdio`, endpoint `None` on memory and `Some` on postgres; mutations of session, HTTP transport, and `endpoint: None` each fail. `open` source pin fails if `.endpoint(` is added. `second_open_on_the_same_memory_store_is_a_new_graph` derives a marker then asserts the second graph empty and recall miss; auto-deriving the marker inside `open` fails it. |
| P3-M2-1 | **HOLDS** | `ensure_layout` (`src/home.rs:189–198`) no longer creates `mooshik.db`. `init_creates_private_usable_layout_and_repairs_modes` asserts `!layout.database.exists()`. Re-adding `ensure_private_file_at(..., "mooshik.db")` fails that test. `HomeLayout.database` remains a path only. |
| P3-M2-2 | **HOLDS** | `resolve_store` (`src/memory/resolve.rs:18–25`) is the only `build_store_with_vector_dim` call. `provision` does not call `resolve_product`. Memory + Gemini dim 1024 with no credentials provisions; routing provision through `resolve_product` fails `provision_does_not_construct_an_embedder`. `open`/`serve` still use `resolve_product` (`open_path_still_constructs_the_embedder`). `config.init_help` names `MOOSHIK_POSTGRES_DSN`; dropping it fails `serve_and_init_help_come_from_text`. |

## Findings

None.

## Hunt (fixes, not findings)

- **DSN identity misuse**: `same_database` only compares; it never prints the identity. Conflict Display is still the secret-free `en.toml` key. Env spelling (including password) is stored and redacted in `config show`.
- **`deny_unknown_fields`**: present on `Config`, `StoreSection`, `EmbedderSection`, vault/session/daemon. Mutation above proves the store-table pin.
- **Lambo SHA**: `Cargo.toml`, `Cargo.lock`, and `/home/nryn/work/lambo` HEAD are `f90a66227b9bff52bdff23bfcffe38b2a1bf7541`.
- **`build_store_with_vector_dim`**: only `resolve_store` (provision). `open`/`serve` stay on `resolve_backends`.
- **`serve()` vs `serve_plan`**: `serve` (`src/memory/ops.rs:59–67`) takes `plan.session`/`plan.agent`, rebuilds `ServeOptions::new` (stdio), then `lambo::mcp::serve`, which publishes via `SessionEndpoint::for_store(&opts.session, &backends.store_cfg)`. `serve_plan` calls `for_store` on `config.store.to_lambo()`, the same `StoreConfig` `resolve_backends` copies into `store_cfg`. Transport and endpoint in the plan are predictions, not a second publisher. Stubbing `serve` to `Ok(())` still yields 66 passing tests because no test calls `serve` — that is the declared plan seam, not a drift in the inputs Lambo actually uses.
- **Home tests**: the layout test asserts the dummy file is **absent**, not present.
- **Secrets in new tests**: passwords appear only as inputs and in `assert!(!…contains("s3cret"))` / `hunter2` checks. Postgres `serve_plan` fixture DSN has no password.

## Requirement verification

Round-1 passes that were already green remain green. Previously partial/failed rows:

| Requirement | Result |
| --- | --- |
| 2. Product backends / dim / model, including partial tables | **Pass** — P2-M2-2 closed. |
| 5. `init_schema` on init; build only preflight | **Pass** — provision is store-only + `init_schema`; no-op fails `MissingDsn`. |
| 7. Dual DSN authorities that name different databases fail closed without echoing secrets | **Pass** — identity, not spelling. Password overlay and `:5432` accepted. |
| 9. M2b holder publishes; `open()` without bind is a lie | **Pass** in source and via `serve_plan` / `open` source pin. Live MCP `serve` still not executed in tests (declared). |

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 66 passed, 0 failed, 0 ignored |
| File-size cap | **PASS** — none over 1000 (largest tracked: `secure_path.rs` 928) |
| Lambo rev `f90a662` in toml/lock/worktree | **PASS** |
| `cargo tree -e features -i lambo` | **PASS** — no `embed-bge`, no `store-sqlite` |

## Mutation score

**16/16** required new-pin mutations failed the named test.

| Pin | Mutation | Result |
| --- | --- | --- |
| Password overlay | identity → string equality | **CAUGHT** |
| Omitted `:5432` | identity → string equality | **CAUGHT** |
| Different hosts refused | `same_database` → `true` | **CAUGHT** |
| No secret in error | `dsn_conflict` text includes `s3cret`/`hunter2` | **CAUGHT** |
| Empty `[store]` is postgres | `default_store_kind` → Memory | **CAUGHT** |
| `[store]` with only DSN is postgres | same | **CAUGHT** |
| Gemini without dim → 1536 | `DEFAULT_EMBED_DIM = 1024` | **CAUGHT** |
| Provision no-op | `provision` → `Ok(())` | **CAUGHT** |
| `serve_plan` session | session hardcoded | **CAUGHT** |
| `serve_plan` stdio | transport HTTP | **CAUGHT** |
| `serve_plan` postgres endpoint | `endpoint: None` | **CAUGHT** |
| `open` must not call `.endpoint(` | add `.endpoint("dead.sock")` | **CAUGHT** |
| Second open write not visible | `open` auto-derives the marker | **CAUGHT** |
| Init does not create `mooshik.db` | restore dummy file create | **CAUGHT** |
| Provision does not construct embedder | `provision` calls `resolve_product` | **CAUGHT** |
| `init_help` names DSN | drop DSN sentence | **CAUGHT** |

Extra hunt (not a claimed pin): stub `serve()` → full lib suite still 66 pass (plan seam).

## Conclusion

**APPROVE.** All five round-1 findings close under independent trace and mutation. No residue introduced by the fixes.

— independent reviewer, 2026-08-25
