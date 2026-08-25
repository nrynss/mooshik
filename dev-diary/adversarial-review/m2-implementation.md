# M2 / M2b implementation

In-process Lambo memory for Mooshik: Postgres + Vertex Gemini as product
backends, schema provision, `Memory` open, and a long-running `serve` that
exposes Lambo's MCP surface with a published session endpoint.

## Files changed

- `Cargo.toml`, `Cargo.lock` — `store-memory` on the pinned lambo rev; direct `tokio` (macros, rt-multi-thread, time).
- `src/lib.rs` — `pub mod memory`.
- `src/config.rs` → `src/config/{mod,overlay,show}.rs` — product defaults, env overlay, DSN redaction.
- `src/memory/{mod,resolve,ops}.rs` — `resolve_product`, `provision`, `open`, `serve`.
- `src/cli.rs` — `init` provisions schema; new `serve` subcommand; `config show` uses redacted TOML.
- `src/text/en.toml` — init/serve help and memory/config error keys.

Not touched: `src/secure_path.rs` (open M1 finding P3-R8-1 left as-is).

## Key decisions

- Product defaults are postgres + gemini + dim 1536 + session/agent `mooshik`. Empty TOML and missing nested tables use Mooshik `Default`, not Lambo's Memory/BGE/1024.
- `Config::default_toml()` writes those defaults and **omits** a DSN.
- Level B single construction: `Config::to_lambo_file()` → `lambo::resolve_backends` → set `promotion_policy = Solo` and `backend_flush_interval` from Mooshik's knob → `Memory::builder().config(..).backends(..)`.
- Flush interval default is Lambo's 1s (`1000` ms). Zero fails at overlay (`ConfigError::ZeroFlush`) and again via `Config::validate` after apply.
- Env overlay uses the same snapshot as M1 (`from_toml_and_env` iterator). Non-empty wins; empty/unset leaves file/default. Mooshik-prefixed names win over the matching `LAMBO_*` names for kind/dim/gemini fields. `load()` passes `env::vars()`, so operators with Lambo env keep working.
- DSN: `MOOSHIK_POSTGRES_DSN` plus Lambo's `LAMBO_POSTGRES_DSN` / `DATABASE_URL`. If both Mooshik and Lambo DSN envs are non-empty and disagree (trimmed string equality), fail closed without echoing either DSN. File DSN vs Lambo env disagreement is the same. Did **not** call `StoreConfig::overlay_env` on process env so unit tests stay isolated from the host.
- Gemini credentials are not reimplemented; the adapter still reads `gemini_credentials` else `GCP_LAMBO_CREDENTIALS` / `GOOGLE_APPLICATION_CREDENTIALS`.
- `mooshik init` still creates home + vault, then `GraphStore::init_schema()`. Postgres without a DSN fails with `memory.missing_dsn` **before** `resolve_backends` (so the operator is told to set `MOOSHIK_POSTGRES_DSN`, not Lambo's missing-DSN sentence). Fixture + memory provisions without a DSN.
- `mooshik serve` loads an existing home, calls `init_schema` (idempotent), then `lambo::mcp::serve(ServeOptions::new(session, agent), backends)` with default stdio transport. `init_tracing()` is inside `memory::serve`. Endpoint bind + `reachable_at` stay in Lambo's serve path (`SessionEndpoint::for_store` after the lease).
- `memory::open` is in-process only and does **not** set `MemoryBuilder::endpoint`. Publishing an address without binding it would advertise a dead socket to a losing `lambo serve`. M2b's holder is `mooshik serve`.
- `open` / `resolve_product` never call `allow_embedding_mismatch`. The live `EmbeddingContract` comes from `ResolvedBackends`.
- `config show` serializes a view that replaces any configured DSN with `***REDACTED***` and omits unset optional fields. It does not `toml::to_string` a live `StoreConfig`.

## Tests added

Config:

- empty TOML + empty env → postgres + gemini + dim 1536
- `default_toml()` round-trips to `Config::default()` and contains no DSN
- non-empty env overlay wins; empty env preserves file
- Mooshik env wins over Lambo env for kind/dim/embedder
- unknown TOML keys rejected (including nested tables)
- dual-DSN env disagreement fails closed without printing a DSN
- file DSN vs `LAMBO_POSTGRES_DSN` disagreement fails closed
- agreeing DSN envs accepted; Lambo DSN fills an omitted file DSN
- zero flush interval fails closed (file and env)
- `config show` redacts a configured DSN and omits DSN when unset

Memory:

- `resolve_product` stamps Solo + configured flush; `allow_embedding_mismatch` is false
- Gemini dim 1024 fails at resolve with 768/1536/3072 in the cause
- Gemini + memory + dummy SA JSON stamps `kind == "gemini"` and `model == Some("gemini-embedding-001")` (no network)
- Postgres without DSN fails before construction
- fixture + memory: `init_schema` then `open` in a temp home; Solo + fixture contract; close
- second `open` on memory store is a new graph (in-RAM store)

CLI:

- `init` / `serve` help strings come from `en.toml`

No live Vertex or Cloud SQL tests were added. Default `cargo test` does not call either.

## Gate results

```
cargo fmt --all -- --check
```

PASS

```
cargo clippy --all-targets --locked -- -D warnings
```

PASS

```
cargo test --locked
```

PASS — 54 tests, 0 failed, 0 ignored

```
wc -l $(git ls-files '*.rs') | awk '$NF != "total" && $1 > 1000'
```

PASS — none over 1000. Largest tracked files: `src/secure_path.rs` 928, `src/vault.rs` 667. New M2 files are all under 400.

## Deliberately not done

- Did not push.
- Did not touch `src/secure_path.rs` / P3-R8-1.
- Did not enable `embed-bge` or `store-sqlite`.
- Did not reimplement the MCP server or ADC.
- Did not put a live DSN in `default_toml()`.
- Did not call `build_store` + `build_embedder` separately.
- Did not add `#[ignore]` live Vertex/Cloud SQL tests.
- Did not use Lambo's private `store_dsn_identity`; dual-DSN comparison is trimmed string equality (fail-closed, stricter than password overlay).
- `memory::open` does not bind or publish a session endpoint.
