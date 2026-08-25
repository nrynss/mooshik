# M3 implementation

OpenAI-compatible companion adapter: streaming `/v1` client, hand-rolled SSE parser,
tool-call protocol, context-window packing with a recall seam, and `mooshik chat`.

## Files changed

- `Cargo.toml`, `Cargo.lock` — reqwest 0.12 (`json`, `stream`, `rustls-tls`, no default
  features); tokio `io-util` / `sync` / `signal` / `net`.
- `src/lib.rs` — `pub mod companion`.
- `src/config/{mod,overlay,show,companion}.rs` — `[companion]` product defaults, env overlay,
  `ApiKey` redaction, `ZeroContextWindow`.
- `src/companion/{mod,cancel,chat,client,pack,session,sse,tools,types}.rs` — client, loop,
  packing, CLI. Test-only `mock.rs` and `loop_tests.rs`.
- `src/cli.rs` — `chat` subcommand dispatch; help from text.
- `src/text/en.toml` — companion help, system prompt, and errors.
- `dev-diary/PLAN.md` — M3 status.

Not touched: `src/secure_path`, `src/vault.rs`, `src/memory`.

## Key decisions

- Product defaults match the local posture: `http://127.0.0.1:8080/v1`, `local-model`,
  window 32768, temperature 0.2, no `api_key`. Empty TOML, missing `[companion]`, and a
  present empty table all use those field defaults (P2-M2-2). A table with only `model`
  still keeps default `base_url` / window / temperature.
- `api_key` is optional. It is omitted from `default_toml()` and `Config::default()`.
  `config show` replaces a set key with `***REDACTED***`. `ApiKey` Debug/Display never
  print the value. Authorization is `Bearer` only when a non-empty key is present.
  Literal `api_key = "local"` is not a default.
- Env overlay: non-empty `MOOSHIK_COMPANION_{BASE_URL,MODEL,API_KEY,CONTEXT_WINDOW,TEMPERATURE}`
  wins; empty/unset leaves file/default. `context_window == 0` fails closed
  (`ConfigError::ZeroContextWindow`).
- Wire JSON types do not `deny_unknown_fields`. Config TOML types do.
- HTTP: `POST {base_url}/chat/completions` with `stream: true`. Trailing slash on
  `base_url` is stripped so both `/v1` and `/v1/` hit `/v1/chat/completions`. Connect
  timeout 10s; whole-request timeout 120s (including the SSE body). Caller cancel
  still aborts the body first. Non-2xx is a static `en.toml` error with no response
  JSON and no credentials.
- Token budget: `ceil(chars/4) + 8` per message. Oldest non-system user/assistant/tool
  groups drop until the packed request fits. The current user turn is never dropped;
  a single turn over the window is `TurnTooLarge`. Dropped turns are handed to
  `RecallInjector`; the default no-op still drops and does not summarize.
- Production `mooshik chat` registers no tools and omits the `tools` request field.
  Tests inject a fake tool. Malformed tool arguments (not a JSON object) become an
  error tool result from `en.toml` and the loop continues. A model that never stops
  calling tools is capped at 8 rounds.
- `mooshik chat` requires an initialized home, loads config, and does not provision
  schema or open `Memory`. Empty stdin lines are ignored; EOF exits 0. Ctrl-C cancels
  an in-flight stream; a second interrupt or EOF exits.

## Tests added

Config:

- empty TOML / missing `[companion]` / empty table → local defaults
- `default_toml()` round-trips, contains `[companion]`, contains no `api_key`
- partial `[companion]` with only `model` keeps default base_url and window
- unknown companion key rejected
- non-empty env overlay wins; empty env preserves file
- zero context_window fails closed
- `config show` redacts `api_key` and never contains the secret
- `ApiKey` Debug/Display/`config show` never print the value

Client / loop (in-process TCP + hand-written HTTP/SSE mock):

- streams content tokens in order
- content then tool_calls assembled from split argument chunks; fake tool runs;
  tool result posted back; second completion streamed
- malformed tool arguments → error tool result, no panic, loop continues
- cancel mid-stream → incomplete assistant not committed; HTTP body aborted
- context pressure: oldest turns absent from the request; injector sees them;
  they are not summarized
- empty tool list omits the `tools` field
- API key is sent as Bearer when set and never appears in Display/error/Debug

CLI:

- `chat` help from `en.toml`

No live model tests. Default `cargo test` does not call a real companion, Vertex, or
Cloud SQL.

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
cargo clippy --locked -- -D warnings
```

PASS

```
cargo test --locked
```

PASS — 89 tests passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`)

```
wc -l $(git ls-files '*.rs') | awk '$NF != "total" && $1 > 1000'
```

PASS — none over 1000. Largest tracked files remain `src/secure_path/mod.rs` and
`src/vault.rs`. New companion files are all under 300 lines.

## Deliberately not done

- Did not push.
- Did not add `async-openai`, `openai-api-rs`, LangChain-style harnesses, or wiremock.
- Did not open `lambo::Memory` from chat or provision schema on `chat`.
- Did not implement M4 tools (`lambo_recall`, `lambo_derive`, `lambo_stats`,
  `run_scratch_script`).
- Did not implement M5 permission gate, vault injection into tools, or egress redaction.
- Did not implement M10 MCP host, TUI, vision, or delegation.
- Did not summarize dropped turns.
- Did not add an `#[ignore]` live companion test.
- `ToolExecutor` / `RecallInjector` are synchronous; M4 can widen them when recall is
  wired.
- Did not treat `api_key = "local"` as a required default.
