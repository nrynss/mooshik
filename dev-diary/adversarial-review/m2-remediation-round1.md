# M2 round-1 remediation

Closes all five round-1 findings (0 P1 / 3 P2 / 2 P3). Lambo identity export
landed first so Mooshik can compare DSNs without reimplementing B's parser.

## Lambo

- Commit `f90a66227b9bff52bdff23bfcffe38b2a1bf7541` on `lambo-for-mooshik`
- **Pushed** to `origin/lambo-for-mooshik`
- Public `lambo::store_dsn_identity` (re-exported from `store` and the crate root)
- `cargo test --locked --lib store::dsn` — 9 passed

Mooshik `Cargo.toml` / `Cargo.lock` pin this rev.

## Closures

### P2-M2-1 — dual-DSN identity

`overlay_dsn` compares `lambo::store_dsn_identity`, not trimmed strings. Env
spelling wins when identities match (password overlay, omitted `:5432`).
Different hosts stay `DsnConflict`. Neither the error nor `config show` contains
the secret.

New tests: `password_overlay_of_one_database_is_accepted`,
`omitted_postgres_port_is_the_same_database`,
`different_hosts_are_a_dsn_conflict_without_echoing_secrets`.

### P2-M2-2 — product field defaults

`StoreSection` / `EmbedderSection` are Mooshik serde views with
`kind = postgres`, `dim = 1536`, default Gemini location/model. Converted to
Lambo types in `to_lambo_file`.

New tests: `empty_store_table_is_postgres`, `store_table_with_only_a_dsn_is_postgres`,
`gemini_table_without_dim_uses_product_default`.

### P2-M2-3 — provision / serve / second-open pins

- `provision(&Config::default())` is `MissingDsn` — a no-op `Ok(())` fails.
- `serve_plan` pins session/agent, stdio, endpoint `None` on memory and `Some`
  on postgres. `open` source is pinned not to call `.endpoint(`.
- Second open: first handle `derive`s a marker concept; second graph is empty
  and recall does not hit it.

### P3-M2-1 — no dummy `mooshik.db`

`ensure_layout` no longer creates the file. `HomeLayout.database` remains as a
path; init tests assert it does not exist.

### P3-M2-2 — init does not construct Gemini

`provision` / `resolve_store` call `build_store_with_vector_dim` only (the one
allowed exception to Level B `resolve_backends`). `open` and `serve` still use
`resolve_product`. Pin: memory store + Gemini dim 1024 with no credentials
provisions; `resolve_product` on the same config still fails. `config.init_help`
names `MOOSHIK_POSTGRES_DSN`.

## Gates

```
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

All PASS. 66 tests, 0 ignored. No tracked `*.rs` over 1000 lines.

## Not done

- Did not push Mooshik.
- Did not touch `src/secure_path.rs` / P3-R8-1.
- Did not add live Vertex / Cloud SQL tests.
- Did not start a live MCP `serve` in tests (plan seam instead).
