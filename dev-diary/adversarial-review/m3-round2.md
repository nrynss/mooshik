# Adversarial review — Mooshik M3, round 2

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: commit `8e168f6c4c0406c7df42e3485793bc1f70fe6d89` on top of `9652f39`, round-1 findings, remediation record.
**Worktree**: `/home/nryn/.grok/worktrees/work-mooshik/subagent-01a0388a-aa7f-7a03-9f87-e7c869818205` @ `8e168f6`
**Verdict**: **APPROVE** — 0 P1 / 0 P2 / 0 P3

## Method

Read round 1 (`dev-diary/adversarial-review/m3-round1.md`, Status: addressed) and `m3-remediation-round1.md`. Re-traced each claimed closure in current source: `src/companion/{client,mock,loop_tests,types,pack,pins,chat,session}.rs`, `src/cli.rs`, `src/config/{companion,overlay}.rs`. Confirmed HEAD `8e168f6c4c0406c7df42e3485793bc1f70fe6d89`.

Mutation-tested every new pin named in round 1's suggestions (break the fix, `cargo test --locked --lib -- --exact <full::path>`, confirm `running 1 test` / `98 filtered out`, restore). Extra hunt: fitting `NoopRecall` summariser; `memory::provision` inside `run_chat`. Transient edits restored; source tree matches HEAD except this file.

## Round-1 closure table

| ID | Verdict | Independent evidence |
| --- | --- | --- |
| P2-M3-1 | **HOLDS** | Mock refuses non-`stream: true` with HTTP 400 (`src/companion/mock.rs:204–209`, `303–308`). Loop tests call `assert_all_streaming()`. Echo first request asserts `tools[0].function.name == "echo"`; follow-up asserts the assistant message immediately before the tool result carries assembled `tool_calls` (`loop_tests.rs:137–156`). Mutations: `stream: false` fails `companion::loop_tests::streams_content_tokens_in_order`; `wire_tools` → `None` fails `content_then_tool_calls_assembled_from_split_chunks`; `WireMessage` always omits `tool_calls` fails the same test. |
| P3-M3-1 | **HOLDS** | First SSE content frame is `Frame::content_openai` (`id` / `object` / `model` / `delta.role` / `usage`). `wire_chunk_types_accept_unknown_openai_fields` parses that envelope and source-pins `#[serde(deny_unknown_fields)]` off the wire-type block (`types.rs:177–206`, test at `234–261`). Mutations: adding the attribute to `ChatChunk` and `ChunkDelta` fails that test **and** `streams_content_tokens_in_order`. |
| P3-M3-2 | **HOLDS** | `current_user_turn_that_exceeds_window_fails` sizes the window to the system message so dropping the current user would succeed (`pack.rs:204–213`). `injector_some_is_packed_and_dropped_turns_stay_out` packs `RECALL_MARKER` with leftover budget. Session context-pressure leaves budget for the marker and asserts it in the captured body (`loop_tests.rs:250–270`). The no-summarize test now budgets a fitting `summary:` message. Mutations: `while !groups.is_empty()` fails TurnTooLarge; discarding `Some(extra)` fails `injector_some` and the session body assertion; `NoopRecall` returning a fitting summary fails `packing_does_not_summarize_dropped_turns`. |
| P3-M3-3 | **HOLDS** | `load_chat_config` only opens the home and loads TOML (`cli.rs:145–148`). `fn chat` calls `run_chat` only. Source pins: `chat_dispatch_does_not_open_memory` (`cli.rs:246–268`) and `run_chat_does_not_open_memory` (`chat.rs:88–97`). `chat_prepare_succeeds_on_default_home_without_dsn` inits a home and parses `default_toml` with empty env (no DSN). `default_config_reaches_companion_without_a_dsn` turns against the mock from `Config::default()`. Mutations: `crate::memory::provision` in `chat()` fails `chat_dispatch_does_not_open_memory`; the same insert in `run_chat` fails `run_chat_does_not_open_memory`. |
| P3-M3-4 | **HOLDS** | `garbage_companion_env_values_fail_closed` pins `CONTEXT_WINDOW=nope` and `TEMPERATURE=inf`/`nope` as `InvalidNumber`. `non_2xx_body_is_not_in_http_status_error` plays HTTP 401 with `s3cret-http-body`; Display equals `companion.http_status` and Debug does not contain the marker. `parallel_tool_calls_merged_by_index` sends index 0 and 1 with split args and asserts both names, arguments, and tool results. Mutations: `parse_u32`/`parse_f64` default on error plus dropped `is_finite` fails the garbage-env test; `panic` with the 401 body fails the HTTP test; merge every delta into slot 0 fails the parallel-tools test. |

## Findings

None.

## Hunt (fixes, not findings)

- **Mock refuse vs assert**: `is_stream_true` 400 and `assert_all_streaming()` both fire on `stream: false`. Either would have been enough for the named test; both are present.
- **Source pins**: `chat_dispatch` splits `fn load_chat_config` / `fn chat(` / `fn block_on`; `run_chat` inspects production text before `#[cfg(test)]`. Inserting `memory::provision` at the round-1 site is red. `deny_unknown_fields` source pin matches `#[serde(deny_unknown_fields)]`, not the doc-comment mention of the phrase.
- **401 body**: discarded via `response.bytes()`; `CompanionError::HttpStatus` remains a unit variant. Secret appears only as input and in `assert!(!contains)`.
- **RECALL_MARKER**: session injector returns `Some(system("RECALL_MARKER"))`; packed request body contains it; dropped turn text does not.
- **File size**: new `pins.rs` is 143 lines; largest companion file is `loop_tests.rs` 321. None over 1000.

## Requirement verification

Round-1 passes that were already green remain green. Previously partial/failed rows:

| Requirement | Result |
| --- | --- |
| 1. Streaming `/v1` client (`stream: true` on the request) | **Pass** — mock 400 + `assert_all_streaming`. |
| 2. Tool-call request shape (advertise tools; assistant `tool_calls` on follow-up; merge by index) | **Pass** — Echo/Pair advertising, follow-up assistant message, parallel index 0/1. |
| 5. Context pressure: current user kept; injector `Some` packed; no summarize | **Pass** — TurnTooLarge when only the user overflows; `RECALL_MARKER` in packed request; fitting summariser would fail. |
| 7. `mooshik chat` does not open Memory; no DSN required | **Pass** — source pins + default home without DSN + default Config against mock. |
| 12. Non-2xx does not dump body / key | **Pass** — 401 fixture. |
| 15. Garbage companion env values fail closed | **Pass** — `nope` / `inf`. |

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 98 passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`) |
| File-size cap | **PASS** — none over 1000 |
| `stream: false` | **CAUGHT** `streams_content_tokens_in_order` |
| `wire_tools` always `None` | **CAUGHT** `content_then_tool_calls_assembled_from_split_chunks` |
| `deny_unknown_fields` on `ChatChunk`/`ChunkDelta` | **CAUGHT** `wire_chunk_types_accept_unknown_openai_fields` |
| `chat()` calls `memory::provision` | **CAUGHT** `chat_dispatch_does_not_open_memory` |

## Mutation score

**12/12** required new-pin mutations failed the named test. Every listed run executed exactly one test (`running 1 test`).

| Pin | Mutation | Result |
| --- | --- | --- |
| `stream: true` | `stream: false` | **CAUGHT** |
| Echo advertises `tools` | `wire_tools` → `None` | **CAUGHT** |
| Follow-up assistant `tool_calls` | `WireMessage` omits them | **CAUGHT** |
| Extra OpenAI fields | `deny_unknown_fields` on `ChatChunk`+`ChunkDelta` | **CAUGHT** (named test and stream fixture) |
| Current user never dropped | `groups.len() > 1` → `> 0` | **CAUGHT** |
| Injector `Some` packed | discard `Some(extra)` | **CAUGHT** (pack + session body) |
| Do not summarize | `NoopRecall` returns a fitting `summary:` | **CAUGHT** |
| Chat does not open Memory | `provision` in `chat()` | **CAUGHT** |
| `run_chat` does not open Memory | `provision` in `run_chat` | **CAUGHT** |
| Garbage env fail closed | parse default + drop `is_finite` | **CAUGHT** |
| 401 body not in error | `panic` with response text | **CAUGHT** |
| Tool deltas by index | merge every delta into slot 0 | **CAUGHT** |

## Conclusion

**APPROVE.** All five round-1 findings close under independent trace and mutation. No residue introduced by the fixes.

— independent reviewer, 2026-08-25
