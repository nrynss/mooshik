# Adversarial review — Mooshik M3, round 1

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: commit `9652f3968ddd0fbe40588e7f643e2112d4927f65` vs origin/main `bf362b3`, implementation record, PLAN § M3 and decision 8, SPEC companion slot.
**Worktree**: `/home/nryn/.grok/worktrees/work-mooshik/subagent-01a0388a-aa7f-7a03-9f87-e7c869818205` @ `9652f39`
**Verdict**: **REQUEST_CHANGES** — 0 P1 / 1 P2 / 4 P3

## Method

Read `dev-diary/adversarial-review/m3-implementation.md`, PLAN M3 / decision 8, SPEC *The companion slot*, and the M3 sources: `src/companion/{mod,cancel,chat,client,pack,session,sse,tools,types,mock,loop_tests}.rs`, `src/config/{mod,overlay,show,companion}.rs`, `src/cli.rs`, `src/text/en.toml`, `Cargo.toml` / `Cargo.lock`. Traced reqwest 0.12.28 (`rustls-tls`, `json`, `stream`, no `native-tls`) and its redirect stripping of `Authorization` on cross-host hops.

Hunted API-key leak, `deny_unknown_fields` on wire vs config, partial `[companion]` defaults, empty `tools: []`, cancel abort / commit, packing summarize / current-turn drop / injector, malformed tool JSON, SSE `[DONE]` / comments / split frames / `tool_calls` index, `base_url` slash, live network in default tests, reqwest 0.13 / native-tls, file-size / `en.toml` / clap derive, library `unwrap`, ToolExecutor / packing pins, temperature-window parse, Bearer when key absent, chat vs `memory::open`, secrets in git.

**Mutation-tested claimed pins** by breaking the fix in the working tree, running `cargo test --locked --lib -- --exact <full::module::path>`, confirming `running 1 test` / `89 filtered out`, and restoring with `git checkout`. Source tree matches HEAD except this file.

Did not treat leftover M1/M2 issues in `secure_path`, `vault.rs`, or `memory` as M3 findings: this diff does not touch those files.

## Findings

### P2-M3-1 — playback mock never consults the request, so `stream: true` and the tool-call request shape are unpinned

**Evidence**: `src/companion/mock.rs:150–181` (`handle_conn` captures the body, then plays the next `Script` regardless of it). `src/companion/client.rs:57–63` (`stream: true`), `src/companion/client.rs:134–151` (`wire_tools`), `src/companion/types.rs:143–174` (`WireMessage` tool_calls). Loop tests assert path, omitted empty `tools`, Authorization, and the **tool result** on round 2 — not the request flags a real `/v1` server needs.

**Reproduction** (transient, restored; each ran exactly one test):

- Set `stream: false` → `companion::loop_tests::streams_content_tokens_in_order` **passed**.
- `wire_tools` always `None` → `content_then_tool_calls_assembled_from_split_chunks` **passed** (and `empty_tool_list_omits_tools_field` still passed).
- `WireMessage` always omits `tool_calls` → `content_then_tool_calls_assembled_from_split_chunks` **passed** (it only looks for `role == "tool"`).

Against llama.cpp / vLLM / OpenAI, `stream: false` returns one JSON object the SSE parser will not read (empty or `InvalidResponse`). A registered tool that is never advertised is never called. A follow-up that posts `role: tool` without the preceding assistant `tool_calls` is often rejected. Production `mooshik chat` currently registers no tools, so the advertising gap is latent until M4; `stream: true` is load-bearing **now**.

This is the M2 P2-M2-3 pattern: the product path is correct in source, and `cargo test` will not notice it going missing. Tool **execution** is pinned (stubbing `executor.execute` fails the same test); the **request** half of the protocol is not.

**Suggestion**: on every captured body, assert `"stream": true`. When Echo is registered, assert `tools[0].function.name == "echo"`. On the second request, assert the assistant message carries `tool_calls` with the assembled id / name / arguments before the tool result. Optionally make the mock refuse non-streaming requests so a `stream: false` regression cannot succeed against playback.

**Status: addressed.** Mock refuses non-`stream: true` with HTTP 400. Loop tests call `assert_all_streaming()`. `content_then_tool_calls_assembled_from_split_chunks` now asserts `tools[0].function.name == "echo"` and that the follow-up assistant message carries assembled `tool_calls` (id/name/arguments) before the tool result. Mutation: `stream: false` fails `streams_content_tokens_in_order`; `wire_tools` → `None` fails `content_then_tool_calls_assembled_from_split_chunks`.

### P3-M3-1 — extra OpenAI-compat fields on wire JSON are unpinned

**Evidence**: `src/companion/types.rs:177–206` correctly omit `deny_unknown_fields` (comment at 177). Mock frames are minimal: `src/companion/mock.rs:37–38` is `{"choices":[{"delta":{"content":text}}]}` — no `id`, `object`, `created`, `model`, or first-delta `role`.

**Reproduction**: add `#[serde(deny_unknown_fields)]` to `ChatChunk` and `ChunkDelta` → `streams_content_tokens_in_order` and `content_then_tool_calls_assembled_from_split_chunks` **passed**. Real OpenAI-compat first deltas include `"role": "assistant"`; chunk envelopes include `id` / `object` / `model`. Tightening the types would break those endpoints while CI stays green. (`ToolCallDelta` is accidentally pinned: `Frame::tool_head` sends `"type": "function"`, so `deny_unknown_fields` there would fail.)

Config TOML **does** `deny_unknown_fields` and that pin is real (`unknown_companion_key_is_rejected`).

**Suggestion**: one fixture chunk with `id`, `object`, `model`, `delta.role`, and `usage`, and a tool_head that already has `type`. Keep `deny_unknown_fields` off the wire types.

**Status: addressed.** First SSE content frame is now an OpenAI envelope (`id`/`object`/`model`/`delta.role`/`usage`). `wire_chunk_types_accept_unknown_openai_fields` parses that JSON and source-pins `#[serde(deny_unknown_fields)]` off the wire types. Mutation: adding it to `ChatChunk` fails that test (and the streaming fixture).

### P3-M3-2 — packing claims about the current user turn and the injector return value are unpinned

**Evidence**: `src/companion/pack.rs:55–57` (`while groups.len() > 1`), `src/companion/pack.rs:73–89` (include `injector.inject` only if it fits), `PER_MESSAGE_OVERHEAD = 8` (`pack.rs:6`). Both packing tests size the window to system + current (session test adds `+ 4`). No injected message can fit, because any message costs at least 8 tokens.

**Reproduction**:

- Change the last-group guard to `while !groups.is_empty()` → both `companion::pack::tests::context_pressure_drops_oldest_turns_and_invokes_injector` and `current_user_turn_that_exceeds_window_fails` **passed**. After dropping the old turn the remainder already fits, so the last group is never a candidate. The TurnTooLarge test uses window `4`, which is also smaller than the system message (~10 tokens), so dropping the current user still errors.
- Call `inject` but discard `Some(extra)` → pack and session context-pressure tests **passed** (injectors in tests always return `None`).
- Make `NoopRecall` return `Some(system("summary: " + dropped))` → `packing_does_not_summarize_dropped_turns` **passed**, because the extra does not fit and is skipped. Observable dropped text is still absent; a summarising injector that *did* fit would not be caught.

Calling the injector **is** pinned (skipping `inject` fails both context-pressure tests). Dropping old turns without putting their text in the request **is** pinned. The stated rules “current user turn is never dropped” and “RecallInjector seam so M4 can wire `lambo_recall`” are not: M4’s `Some(recall)` path is dead to `cargo test`.

**Suggestion**: (1) system fits, current user does not → `TurnTooLarge`, packed/request must not succeed without that user text. (2) leftover budget ≥ one injected message; a recording injector returns `Some(user("RECALL_MARKER"))`; packed messages / captured body contain the marker and still omit dropped turns.

**Status: addressed.** `current_user_turn_that_exceeds_window_fails` now sizes the window to the system message only. `injector_some_is_packed_and_dropped_turns_stay_out` packs `RECALL_MARKER`. The session context-pressure test leaves budget for the marker and asserts it in the captured body. Mutation: `while !groups.is_empty()` fails TurnTooLarge; discarding `Some(extra)` fails `injector_some`.

### P3-M3-3 — `mooshik chat` does not open Memory / does not need a DSN is unpinned

**Evidence**: `src/cli.rs:145–149` (`open_existing_root`, `Config::load_at`, `run_chat`). `src/companion/chat.rs:21–23` builds `Session::new` from `CompanionConfig` only. Correct, and `chat.rs` does not import `memory`. The only chat test is `cli::tests::chat_help_comes_from_text`.

**Reproduction**: insert `let _ = crate::memory::provision(&config);` into `chat()` → `chat_help_comes_from_text` **passed**. `provision` without a DSN is `MissingDsn` (M2 pin). That regression would make `mooshik chat` require Postgres before talking to a local `/v1` endpoint, which is exactly what M3 promised not to do.

**Suggestion**: a dispatch test with an initialized home, default config (no DSN), and the in-process mock as `base_url`. Chat must reach the companion (or fail `Unreachable` / `HttpStatus`), not `memory.missing_dsn`. Alternatively a compile-time/module test that `chat` / `run_chat` do not call `memory::{open,provision,serve}`.

**Status: addressed.** Source pins on `load_chat_config` / `fn chat` / `run_chat` reject `memory::` and `provision`. `chat_prepare_succeeds_on_default_home_without_dsn` loads an initialized home's `default_toml` without a DSN. `default_config_reaches_companion_without_a_dsn` turns against the mock from `Config::default()`. Mutation: inserting `memory::provision` into `chat()` fails `chat_dispatch_does_not_open_memory`.

### P3-M3-4 — garbage companion env values and the non-2xx HTTP path are unpinned

**Evidence**: `src/config/overlay.rs:195–206` (`parse_u32` / `parse_f64`, `is_finite` on temperature), `src/config/overlay.rs:224–229`. `src/companion/client.rs:73–76` drains the error body and returns a static `HttpStatus`. Zero window **is** pinned. Non-numbers and `inf` are not. No loop test uses `Script { status: 401, .. }`.

**Reproduction**:

- `parse_u32` / `parse_f64` `unwrap_or(32768/0.2)` on failure → `zero_context_window_fails_closed`, `companion_env_overlay_wins_and_empty_preserves_file`, and `unknown_and_malformed_values_are_rejected` **passed** (`nope` is only tested for embed dim).
- Drop the `is_finite` check → overlay test **passed**.
- `panic!("http error: {body}")` on non-2xx → `api_key_never_appears_in_client_errors` **passed** (that test hits `127.0.0.1:1`, connection refused, never HTTP). A 401 body that echoed the key would not be caught.

**Suggestion**: pin `MOOSHIK_COMPANION_CONTEXT_WINDOW=nope` and `TEMPERATURE=inf` as `InvalidNumber`. Pin a 401 script whose body contains a marker/secret; `CompanionError::HttpStatus` Display / Debug must not contain it.

`delta.tool_calls` **by index** is in the same bucket: `merge_tool_deltas` (`client.rs:200–218`) is only exercised at `index: 0`. Forcing every delta into slot 0 → `content_then_tool_calls_assembled_from_split_chunks` **passed**. Add two parallel tool heads (index 0 and 1) with split argument chunks and assert both names/args and both tool results.

**Status: addressed.** `garbage_companion_env_values_fail_closed` pins `CONTEXT_WINDOW=nope` and `TEMPERATURE=inf` as `InvalidNumber`. `non_2xx_body_is_not_in_http_status_error` uses a 401 script whose body contains `s3cret-http-body`; Display/Debug stay the static `en.toml` key. `parallel_tool_calls_merged_by_index` sends index 0 and 1 with split args and asserts both names, arguments, and tool results. Mutations: drop `is_finite` / `parse_u32` default, panic with the 401 body, and merge every delta into slot 0 each fail the named test.

## Requirement verification

| Requirement | Result |
| --- | --- |
| 1. Hand-rolled OpenAI-compat `/v1` streaming client (SSE parser, not a harness; no async-openai) | **Pass** in source (`reqwest` 0.12, `SseParser`, no provider SDK). **Partial** pin: response streaming is tested; `stream: true` on the request is not (P2-M3-1). |
| 2. Message loop + tool-call protocol | **Pass** in source. Execute / malformed JSON / finish_reason `tool_calls` / argument concatenation are pinned. Request advertising and assistant `tool_calls` on the follow-up are not (P2-M3-1). Index merge for `n>0` unpinned (P3-M3-4). |
| 3. Partial-stream cancellation | **Pass**. Cancel during the chunk loop returns `Cancelled`, does not commit an assistant, aborts the HTTP body (M3b, M36). |
| 4. Tool call arriving mid-stream (content then tool_calls deltas) | **Pass**. `content_then_tool_calls_assembled_from_split_chunks`; breaking `finish_reason` or argument `push_str` fails it. |
| 5. Context-window pressure: drop old turns, do not summarize; `RecallInjector` seam | **Partial**. Oldest turns are dropped and `inject` is called (M4). Current-user guard, injector `Some` inclusion, and a fitting summariser are unpinned (P3-M3-2). |
| 6. Malformed tool JSON → error tool result, loop lives, no panic | **Pass**. `parse_tool_object` unwrap or skipping the error arm fails `malformed_tool_arguments_yield_error_result_and_loop_continues`. |
| 7. `mooshik chat` persistent loop; does not open Lambo Memory; no DSN required | **Pass** in source (`open_existing_root` + `run_chat`; no `memory::`). **Unpinned** (P3-M3-3). Help strings say so. |
| 8. `[companion]` product defaults on partial tables | **Pass**. Empty TOML, missing table, empty table, and `[companion] model = ...` keep `http://127.0.0.1:8080/v1`, `local-model`, 32768, 0.2, no key (M1, M32, M37). |
| 9. reqwest 0.12 rustls-tls json stream, default-features false | **Pass**. `Cargo.toml` + `Cargo.lock` `reqwest 0.12.28` once; `cargo tree -e features -i reqwest` shows `rustls-tls` / `json` / `stream`; no `native-tls`. |
| 10. Strings in `en.toml`; clap builder; file-size cap 1000 | **Pass**. Companion errors/help/system prompt from `en.toml`. `cli::command()` is clap builder. No tracked `*.rs` >1000 (largest new companion file: `loop_tests.rs` 283). |
| 11. Default `cargo test` does not hit a live model | **Pass**. Mock binds `127.0.0.1:0`. The only non-mock complete() uses `127.0.0.1:1` (refused). 89 passed, 1 ignored (`live_postgres_and_gemini_round_trip`). |
| 12. API key never in Display / error / `config show` / Debug | **Pass** for the paths traced. `ApiKey` Debug/Display, `ShowCompanion` view, `CompanionError` unit variants, non-2xx body discarded. Pins hold (M7, M12, M22). Non-2xx dump remains unpinned (P3-M3-4). |
| 13. Decision 8: SSE parser, not a harness | **Pass**. `[DONE]`, CRLF split frames, comment lines (via `data:`-only), tool argument chunks. |
| 14. `base_url` trailing slash; Authorization omitted when key absent; `default_toml` has no `api_key` | **Pass**. M19, M10, M11. |
| 15. Zero context window fails closed | **Pass**. File and env (M9). Non-numeric env unpinned (P3-M3-4). |
| 16. Do not treat leftover M1/M2 issues as M3 findings unless this diff touched them | **Honoured**. |

Other hunt notes, not findings: library companion paths have no `unwrap` on user/network input (`chat.rs` Mutex uses `into_inner` on poison; `show.rs` `expect` is serialize-of-view, same shape as M2). `CompanionClient` has no `Debug`. Bearer is `Zeroizing`. Redirects strip `Authorization` cross-host (reqwest 0.12.28). CI clippy is `cargo clippy -- -D warnings` (not `--all-targets`); both forms passed here. Hunt-list SSE comment skip is redundant with ignoring non-`data:` lines (removing the `:` branch stayed green; comments still produce no events).

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo clippy --locked -- -D warnings` (CI form is without `--locked` / `--all-targets`) | **PASS** |
| `cargo test --locked` | **PASS** — 89 passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`) |
| File-size cap (`wc -l` tracked `*.rs`, fail >1000) | **PASS** — none over 1000 |
| reqwest 0.12 pin / rustls / no native-tls | **PASS** — single `reqwest 0.12.28`, features `json`+`stream`+`rustls-tls` |
| `stream: false` | **still green** (P2-M3-1) |
| `wire_tools` always `None` | **still green** (P2-M3-1) |
| `deny_unknown_fields` on `ChatChunk`/`ChunkDelta` | **still green** (P3-M3-1) |
| `chat()` calls `memory::provision` | **still green** (P3-M3-3) |

## Mutation score

**23/34** claimed-pin mutations failed the named test after the fix was broken. Every listed run executed exactly one test (`running 1 test`, `89 filtered out`).

Vacuous (test still passed):

| Claimed pin | Mutation | Why it stayed green |
| --- | --- | --- |
| Streaming `/v1` client (`stream: true`) | `stream: false` | Mock always replies SSE (P2-M3-1). |
| Tool-call protocol advertises tools | `wire_tools` → `None` | Mock scripts tool_calls anyway; test checks the tool **result**. |
| Follow-up carries assistant `tool_calls` | `WireMessage` omits them | Test only finds `role == "tool"`. |
| Wire types accept extra OpenAI keys | `deny_unknown_fields` on `ChatChunk`/`ChunkDelta` | Mock frames have no extra keys (P3-M3-1). |
| Current user turn never dropped | `groups.len() > 1` → `> 0` | After dropping old turns the remainder already fits; TurnTooLarge window is also `< system` (P3-M3-2). |
| Injector return is packed | discard `Some(extra)` | Tests inject `None`; leftover budget `< 8` (P3-M3-2). |
| Do not summarize | `NoopRecall` returns a `summary:` message | Injection skipped as over budget (P3-M3-2). |
| `mooshik chat` does not open Memory | `chat()` calls `provision` | Only help text is tested (P3-M3-3). |
| Non-numeric window/temperature fail closed | `parse_*` defaults on error; drop `is_finite` | Only `0` and well-formed numbers are tested (P3-M3-4). |
| Non-2xx does not dump body / key | `panic` with response text | No test hits HTTP 4xx/5xx (P3-M3-4). |
| `delta.tool_calls` by index | always merge into slot 0 | Sole tool-call test uses `index: 0` (P3-M3-4). |

Caught (selection): product field defaults on empty/partial `[companion]`, unknown companion key, env overlay win/empty-preserve, zero window, `default_toml` omits `api_key`, `config show` / Display / Debug redaction, empty `tools` omitted, no Bearer when key absent, `ToolExecutor::execute`, malformed tool JSON, `finish_reason: tool_calls`, argument chunk concat, cancel abort + no commit, injector **call**, `[DONE]`, SSE split frames, trailing-slash URL.

Not counted as vacuous: removing the post-loop `is_cancelled` check (the `select!` arm is the pin); removing the SSE `:` branch (non-`data:` lines are already ignored).

## Conclusion

**REQUEST_CHANGES.** The adapter is real: hand-rolled SSE, product defaults on partial `[companion]` tables (the M2 P2-M2-2 lesson), redacted `ApiKey`, no Bearer on the local default, empty tools omitted, cancel aborts the body and does not commit a partial assistant, malformed tool JSON becomes an `en.toml` tool result, old turns drop without a summariser in the packed request, reqwest is 0.12 rustls, and default tests never touch a live model. Tool **execution** is pinned.

One P2 blocks approval: the in-process mock is a playback script, so `stream: true` and the tool-call **request** shape can vanish without a red test. Four P3s are missing pins this project does not ship with: extra OpenAI fields, the packing seam M4 will need, `mooshik chat` vs Memory, and fail-closed garbage env / non-2xx / multi-index tool deltas.

— independent reviewer, 2026-08-25

## Closures (remediator)

| ID | Status | Pin |
| --- | --- | --- |
| P2-M3-1 | **addressed** | `stream: true` on every captured body; mock 400 if not; Echo `tools[0]`; follow-up assistant `tool_calls` |
| P3-M3-1 | **addressed** | OpenAI envelope fixture + source pin off `deny_unknown_fields` on wire types |
| P3-M3-2 | **addressed** | system-fits/user-does-not TurnTooLarge; injector `Some(RECALL_MARKER)` packed and in the request |
| P3-M3-3 | **addressed** | `chat`/`load_chat_config`/`run_chat` source pins; default home has no DSN; default Config reaches mock |
| P3-M3-4 | **addressed** | `nope`/`inf` → `InvalidNumber`; 401 body discarded; multi-index tool deltas |
