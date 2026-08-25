# Adversarial review — Mooshik M2/M2b, round 1

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: commit 5ed3e68 vs origin/main, implementation record, PLAN M2/M2b with operator Postgres+Gemini override.
**Worktree**: `/home/nryn/.grok/worktrees/work-mooshik/subagent-01a037b8-8dfd-7ea1-8816-f2b12b352e5e` @ 5ed3e68
**Verdict**: **REQUEST_CHANGES** — 0 P1 / 3 P2 / 2 P3

## Method

Read `dev-diary/adversarial-review/m2-implementation.md`, PLAN M2/M2b, and the M2 sources: `src/config/{mod,overlay,show}.rs`, `src/memory/{mod,resolve,ops}.rs`, `src/cli.rs`, `src/text/en.toml`, `src/lib.rs`, `Cargo.toml`. Traced Lambo at `/home/nryn/work/lambo` @ `94cbf52` (matches `Cargo.toml` / `Cargo.lock`) for `resolve_backends`, `StoreConfig::overlay_env` / `store_dsn_identity`, `mcp::serve` + `SessionEndpoint::for_store`, the Gemini dim guard in `build_gemini_embedder`, `MemoryBuilder::{backends,config,build}` (`preflight_schema` only), and `GraphStore::init_schema`.

Hunted DSN leak, dual-DSN identity, `open()` endpoint lie, Solo, Gemini construction, host-env isolation, `mooshik.db`, stdout-under-stdio, and serde defaults on inlined Lambo types. Transient probes and mutations were restored; source tree matches HEAD except this file.

Did not treat leftover M1 P3-R8-1 (`secure_path.rs` staging cleanup) as an M2 finding: this diff does not touch that file.

**Mutation-tested claimed pins** by breaking the fix in the working tree, running the named test, and restoring. A first pass used `cargo test -- --exact` with a bare test name and ran **zero tests** (vacuous green); the score below is from the second pass, which actually executed each test. Extra mutations (not claimed pins) stubbed `serve` and `provision`.

## Findings

### P2-M2-1 — dual-DSN authority uses trimmed string equality, so one database with two spellings fails closed

**Evidence**: `src/config/overlay.rs:143–158`. File DSN vs `LAMBO_POSTGRES_DSN` / `DATABASE_URL`, and `MOOSHIK_POSTGRES_DSN` vs those Lambo envs, compare `trim()`ed strings. Lambo's `StoreConfig::overlay_env` (`/home/nryn/work/lambo/src/store/mod.rs:944–995`) compares `store_dsn_identity`: password stripped, omitted port is 5432, `postgres`/`postgresql` folded. The implementation record admits the private helper was not used.

**Reproduction**: `from_toml_and_env` with file `postgres://app@host/db` and `LAMBO_POSTGRES_DSN=postgres://app:s3cret@host/db` returns `ConfigError::DsnConflict`. Lambo identity says those name one database and the env spelling should win (password overlay). The same refusal fires for `postgres://u@host/db` vs `postgres://u@host:5432/db`. The dual-env test (`dual_dsn_envs_that_disagree_fail_closed`) only feeds two different userinfo strings, so it stays green without identity. Error text does not echo a DSN (`en.toml` `config.dsn_conflict`); that half is sound.

This is fail-closed in the conservative direction (it will not silently open two different databases). It is still an env/DSN authority bug: the documented Lambo secret-handling shape is refused, and two spellings of one store look like a conflict.

**Suggestion**: compare database identity, not spelling. Export or wrap Lambo's identity helper (do not reimplement the parser). Keep printing via the existing secret-free `DsnConflict` string. Add pins: password overlay accepted; omitted `:5432` accepted; two different hosts still refused; neither error nor `config show` contains the secret.

### P2-M2-2 — a present `[store]` table without `kind` silently selects the Memory test double

**Evidence**: `src/config/mod.rs:163–164` uses `#[serde(default = "default_store")]` only when the **table is missing**. When `[store]` is present, serde uses Lambo `StoreConfig`'s field defaults. `StoreKind::default()` is `Memory` (`/home/nryn/work/lambo/src/store/mod.rs:856–864`). Mooshik's product default is `default_store()` → Postgres (`src/config/mod.rs:253–259`). Session/daemon tables do this correctly with per-field `default = "default_session_id"` / `default_flush_interval_ms`; the inlined Lambo structs do not.

`overlay_dsn` then returns immediately for non-Postgres (`src/config/overlay.rs:137–139`), so a postgres DSN sitting in that table is ignored and dual-DSN checks never run. `resolve_product` skips `MissingDsn` and `resolve_backends` builds `MemoryStore`. Writes are in-RAM and vanish on exit.

Empty TOML and a missing `[store]` table still hit `Config::default()` / `default_store()` and are pinned (`empty_toml_resolves_to_postgres_gemini_1536`). First-run `default_toml()` includes `kind = "postgres"`. The gap is any operator-edited `[store]` that names a DSN (or is empty) without repeating `kind`.

Sibling, fail-closed: `[embedder] kind = "gemini"` without `dim` deserializes `dim = 1024` (Lambo `default_embed_dim`), then Gemini resolve refuses {768,1536,3072}. Wrong, but loud.

**Reproduction**: `Config::from_toml_and_env("[store]\ndsn = 'postgres://prod:s3cret@localhost/prod'\n", [])` yields `kind = Memory` and keeps the DSN unused. `resolve_product` succeeds against Memory.

**Suggestion**: give Mooshik its own serde view of store/embedder with product field defaults (`kind = postgres`, `dim = 1536`, Gemini model/location), then convert to Lambo types. Pin `[store]` with only a DSN, empty `[store]`, and `[embedder] kind = "gemini"` without dim.

### P2-M2-3 — M2b serve, schema provision, and in-RAM graph identity are unpinned

**Evidence**:

- `src/memory/ops.rs:34–43` (`serve`) and `src/memory/ops.rs:25–31` (`provision`) are the product holder and `init_schema` paths. `src/cli.rs:123–137` is the only caller of each.
- Claimed pin `second_open_on_the_same_memory_store_is_a_new_graph` (`src/memory/ops.rs:82–90`) asserts only `embedding_contract().kind == "fixture"` on the second handle. It would pass against a durable store.

**Reproduction** (transient, restored):

- Replace `serve` with `Ok(())` → `cargo test --locked --lib` **54 passed**.
- Replace `provision` with `Ok(())` → **54 passed**.
- No-op comment inside `open` → `second_open_on_the_same_memory_store_is_a_new_graph` still passed.

`open()` itself is honest: it does not call `MemoryBuilder::endpoint` (`src/memory/ops.rs:10–21`). Lambo `mcp::serve` does `SessionEndpoint::for_store` then `builder.endpoint(...)` (`/home/nryn/work/lambo/src/mcp/serve.rs:1520,780–809`). That wiring is trace-true and untested from Mooshik. A library `open()` that published without binding would be a lie; this code does not do that. The gap is that nothing in `cargo test` would notice if `serve` stopped publishing, or if `init_schema` stopped running.

**Suggestion**: pin `init_schema` with a store that records the call (or a postgres test double). Pin `serve` at least through `ServeOptions` + the Lambo serve entry (fixture + memory, stdio or bound endpoint, tracing on stderr only). Make `second_open` write on the first handle and assert the second graph does not see it.

### P3-M2-1 — `init` still plants an empty `mooshik.db` beside a Postgres product store

**Evidence**: `src/home.rs:24` and `src/home.rs:197` create `~/.mooshik/mooshik.db` as a private empty file. M2 did not touch `home.rs`. The product store is Postgres (`default_toml`, `default_store`). Nothing in M2 reads `HomeLayout.database`. PLAN M1 still draws that file as layout.

**Reproduction**: `HomeLayout::init` leaves a 0600 empty `mooshik.db`. Tests still assert `layout.database.is_file()`.

**Suggestion**: stop creating the dummy file, or document it as unused until a local store exists. Do not let operators treat it as the workspace graph.

### P3-M2-2 — `mooshik init` schema provision requires a live Gemini embedder config

**Evidence**: `provision` (`src/memory/ops.rs:25–31`) calls `resolve_product` → `lambo::resolve_backends` → `build_embedder`. Lambo provision uses `resolve_store_only` and does not construct an embedder. After `MOOSHIK_POSTGRES_DSN` is set, `mooshik init` therefore needs Gemini credentials (config path or `GCP_LAMBO_CREDENTIALS` / `GOOGLE_APPLICATION_CREDENTIALS`) even though it only runs DDL. `init_help` does not say so; `memory.serve_after_help` does. Postgres without a DSN still fails first with `memory.missing_dsn` (`src/memory/resolve.rs:9–18`), which is the right order.

**Suggestion**: provision via store-only resolution (or equivalent) so `init` needs a DSN, not Vertex ADC. Keep full `resolve_backends` on `open` / `serve`. Mention DSN and credentials in `config.init_help` if init still constructs an embedder.

## Requirement verification

| Requirement | Result |
| --- | --- |
| 1. Wire `Memory` through `MemoryBuilder`: session, agent, store, embedder, contract, flush, scoring/cadence via `.config(backends.config)` | **Pass** (trace). `open` sets `.config(lambo_config).backends(backends)`. Scoring is Lambo's default; flush/Solo are stamped on `backends.config` before the builder. Mutation M18: dropping `.config(...)` fails `fixture_memory_provisions_and_opens`. |
| 2. Product backends `store-postgres` + `embed-gemini`, dim in {768,1536,3072}, default 1536, model `gemini-embedding-001` | **Pass** for empty TOML / `default_toml` / `Config::default()`. **Fail** for partial `[store]` (P2-M2-2). Gemini 1024 refused at resolve (mutation M15). |
| 3. `default-features = false`; no `embed-bge`, no `store-sqlite`; `store-memory` + `embed-fixture` are test doubles | **Pass**. `Cargo.toml` features and `cargo tree -e features -i lambo` show only `store-postgres`, `store-memory`, `embed-gemini`, `embed-fixture` (plus `sqlx` via postgres). |
| 4. Level B: `resolve_backends(LamboFile)`, not separate `build_store`+`build_embedder` | **Pass**. Single call in `resolve_product` (`src/memory/resolve.rs:20`). |
| 5. `init_schema()` on init; `MemoryBuilder::build` only `preflight_schema` | **Pass** in source (`provision`/`serve` call `init_schema`; Lambo `build_attach` preflights at `memory.rs:821`). **Unpinned** (P2-M2-3). |
| 6. Promotion policy Solo (C2) | **Pass**. Set at `resolve.rs:21`; Lambo `Config::validate` no longer refuses Solo (C2 landed). Mutations M12/M18 catch dropping it. |
| 7. Env overlay: non-empty wins; empty leaves file. Dual DSN authorities that name different databases fail closed without echoing secrets | **Partial**. Overlay order and empty-env preservation are pinned (M3–M5, M4). Dual-env and file-vs-Lambo refusals do not echo DSNs (M7, M8). Comparison is spelling, not identity (P2-M2-1). File vs `MOOSHIK_POSTGRES_DSN` is env-wins even across databases (stated overlay rule, not Lambo E2E-F2). |
| 8. DSN never in `default_toml`; never in `config show`; never in error strings | **Pass** for Mooshik-owned strings. `default_toml` omits `dsn` (M2). `redacted_toml` uses a view, not `toml::to_string` of `StoreConfig` (M10/M11). `DsnConflict` / `MissingDsn` text has no URL. `StoreConfig`'s `Debug` redacts. `main` prints `{err:#}` so **Lambo** causes can appear; no password leak proven on the parse/connect messages traced. |
| 9. M2b: long-running holder serves Lambo MCP and publishes a session endpoint. `open()` without bind is a lie | **Pass** in source: `open` does not set `endpoint`; `serve` calls `lambo::mcp::serve`, which derives `SessionEndpoint::for_store` and publishes on acquire. `init_tracing` writes stderr, not stdout. **Unpinned** (P2-M2-3). |
| 10. Default `cargo test` must not hit Vertex or Cloud SQL | **Pass**. Gemini construction uses a dummy SA JSON; token mint is lazy (`gcp_auth::GoogleOAuthTokenSource::new`). Postgres path returns `MissingDsn` before `PgStore::new`. 54 tests, 0 ignored, no live adapters. |
| 11. File-size cap 1000; user-facing strings in `src/text/en.toml`; clap builder API | **Pass**. No tracked `.rs` file >1000 (largest: `secure_path.rs` 928, `vault.rs` 667; new M2 files all <400). Help/errors from `en.toml`. `command()` is clap builder, not derive. |
| 12. Do not treat leftover M1 P3-R8-1 as an M2 finding unless this diff touched it | **Honoured**. `secure_path.rs` not in 5ed3e68. |

Other hunt notes, not findings: `allow_embedding_mismatch` is never set (M14). Host `LAMBO_*` cannot leak into `from_toml_and_env` tests; `Config::load` / `load_at` pass `env::vars()` so production overlay is not forgotten. `mooshik serve` creates its own multi-thread runtime (`src/cli.rs:139–148`).

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 54 passed, 0 failed, 0 ignored (lib 54 + bin 0 + doc 0) |
| File-size cap (`wc -l` tracked `*.rs`, fail >1000) | **PASS** — none over 1000 |
| `cargo tree -e features -i lambo` | **PASS** — no `embed-bge`, no `store-sqlite` |
| Lambo rev `94cbf52` | **PASS** — worktree HEAD, `Cargo.toml`, `Cargo.lock` agree |
| Serve body stubbed to `Ok(())` | **54 still pass** (P2-M2-3) |
| Provision body stubbed to `Ok(())` | **54 still pass** (P2-M2-3) |

## Mutation score

**20/22** claimed-pin mutations failed the named test after the fix was broken.

Vacuous (test still passed):

| Claimed pin | Mutation | Why it stayed green |
| --- | --- | --- |
| Gemini stamps `model == Some("gemini-embedding-001")` | `default_embedder` leaves `gemini_model: None` | Lambo's adapter default still stamps `gemini-embedding-001`. Empty-TOML **does** pin Mooshik's default model field; this test does not. Construction-without-network still holds. |
| Second open on memory store is a new graph | No-op in `open` | Test never compares the two graphs (P2-M2-3). |

Caught (selection): product defaults, `default_toml` DSN omission, env overlay win/preserve/precedence, unknown keys, dual-DSN string conflict, DSN redaction, Solo, flush stamp, `allow_embedding_mismatch == false`, Gemini dim 1024, early `MissingDsn`, help strings, `MemoryBuilder::config` on `open`, Lambo DSN fill, agreeing DSN envs.

## Conclusion

**REQUEST_CHANGES.** The Level B wiring, Solo stamp, early missing-DSN path, `open()`-does-not-publish, stderr tracing, feature set, and most overlay/redaction pins are real. Three P2s block approval: dual-DSN spelling vs identity, serde field defaults that fail open to Memory, and an M2b/schema surface that `cargo test` will not notice going missing. Two P3s (dummy `mooshik.db`, init coupled to Gemini) should close in the same pass.

— independent reviewer, 2026-08-25

## Round 1 closures

Findings text above is unchanged. Closures:

| ID | Status | Response |
| --- | --- | --- |
| P2-M2-1 | **fixed** | Lambo `store_dsn_identity` is public (`f90a662`, pushed). Overlay compares identity. Password overlay and omitted `:5432` accepted (env spelling wins). Different hosts `DsnConflict` without echoing secrets. |
| P2-M2-2 | **fixed** | Mooshik `StoreSection` / `EmbedderSection` carry product field defaults (`kind = postgres`, `dim = 1536`, gemini location/model). Empty `[store]`, `[store]` with only a DSN, and `[embedder] kind = "gemini"` without dim are pinned. |
| P2-M2-3 | **fixed** | `provision` without DSN is `MissingDsn` (stubbing to `Ok(())` fails). `serve_plan` pins stdio, session/agent, endpoint Some for postgres / None for memory; `open` is pinned not to call `.endpoint(`. Second open writes then asserts the new graph is empty. |
| P3-M2-1 | **fixed** | `HomeLayout::init` no longer creates `mooshik.db`. Tests assert the file is absent. |
| P3-M2-2 | **fixed** | `provision` uses `build_store_with_vector_dim` only. Memory + invalid Gemini (no creds, dim 1024) still provisions. `open`/`resolve_product` still constructs the embedder. `init_help` names `MOOSHIK_POSTGRES_DSN`. |
