# M3 round-1 remediation

Closes all five round-1 findings (0 P1 / 1 P2 / 4 P3). Pins that were green
under the reviewer's mutations now fail the named test.

## Closures

### P2-M3-1 — `stream: true` and tool-call request shape

The playback mock refuses a body whose `stream` is not `true` (HTTP 400 JSON,
not SSE). Loop tests call `MockServer::assert_all_streaming()`. When Echo is
registered, the first request must advertise `tools[0].function.name == "echo"`.
The second request must carry the assistant `tool_calls` (assembled id, name,
arguments) immediately before the tool result.

New/extended tests: `streams_content_tokens_in_order`,
`content_then_tool_calls_assembled_from_split_chunks`.

### P3-M3-1 — extra OpenAI-compat wire fields

`Frame::content_openai` emits `id`, `object`, `model`, `delta.role`, and
`usage`. `wire_chunk_types_accept_unknown_openai_fields` parses that JSON and
source-pins `#[serde(deny_unknown_fields)]` off the wire-type block. Wire types
still omit the attribute.

### P3-M3-2 — current user turn and injector `Some`

`current_user_turn_that_exceeds_window_fails` sizes the window to the system
message so dropping the current user would succeed and is now a red test.
`injector_some_is_packed_and_dropped_turns_stay_out` returns
`Some(system("RECALL_MARKER"))` with leftover budget. The session
context-pressure test does the same and asserts the marker in the captured
body while dropped turns stay out. The no-summarize test now leaves enough
budget that a fitting summariser would appear.

### P3-M3-3 — `mooshik chat` does not open Memory

`load_chat_config` only opens the home and loads TOML. Source pins on
`load_chat_config`, `fn chat`, and `run_chat` reject `memory::` / `provision`.
`chat_prepare_succeeds_on_default_home_without_dsn` inits a home and parses
`default_toml` with empty env (no DSN). `default_config_reaches_companion_without_a_dsn`
turns against the mock from `Config::default()`.

### P3-M3-4 — garbage env, non-2xx body, multi-index tools

`garbage_companion_env_values_fail_closed` pins
`MOOSHIK_COMPANION_CONTEXT_WINDOW=nope` and `MOOSHIK_COMPANION_TEMPERATURE=inf`
as `InvalidNumber`. `non_2xx_body_is_not_in_http_status_error` plays HTTP 401
with `s3cret-http-body`; Display/Debug are the static `en.toml` key.
`parallel_tool_calls_merged_by_index` sends index 0 and 1 with split argument
chunks and asserts both names, arguments, and tool results.

## Gates

```
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

All PASS. 98 tests passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`).
No tracked `*.rs` over 1000 lines. New pins live in `src/companion/pins.rs` so
`loop_tests.rs` stays under the soft target.

## Not done

- Did not push.
- Did not add an `#[ignore]` live companion test.
- `ToolExecutor` / `RecallInjector` remain synchronous.
