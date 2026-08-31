# M12e implementation — the pane converses

## Files changed

- `src/cli/tui_cmd.rs` — `live()` / `converse()` compose the session over the pane, spawn turns, drain tokens. `Pane::tools` takes a `Diagnostics` sink. `--demo` still never opens Memory.
- `src/tui/mod.rs` — `TurnDrive` seam; `run` takes it (`None` on `--demo`); event loop drains every pass and polls `STREAM` (16 ms) while a turn is in flight, `TICK` (250 ms) when idle.
- `src/tui/app.rs` — `Action::Send` moves the draft; `Action::Cancel`; `append_token` / `finish_turn` / `note`.
- `src/tui/app_tests.rs` — Send / empty-draft / token+refresh / failure / Esc pins. Replaces `sending_does_not_discard_the_draft`.
- `src/tui/input.rs` — idle `Esc` is Quit; in-flight `Esc` is Cancel.
- `src/companion/chat.rs` — `compose_session` is `pub(crate)`.
- `src/companion/mod.rs` — re-exports `compose_session`; `mock` is `pub(crate)` under test.
- `src/tools/diagnostics.rs` — new. Cloneable execute-time sink; default stderr.
- `src/tools/mod.rs` — `MemoryTools` / `compose_chat_stack` / `executor_over_memory` take the sink. CLI `executor_for_chat` still prints assembly notices.
- `src/tools/permissions.rs` — gate panic goes through the sink.
- `src/mcp_host/mod.rs` — spawn / dispatch / panic sites go through the sink.
- `src/text/en.toml` — `tui.turn_pending`.
- tests in `src/tools/tests.rs`, `src/tools/recall.rs`, `src/mcp_host/tests.rs`.

No file is over 1000 lines (`tui/mod.rs` 890, `tui_cmd.rs` 791).

## What was called from the seam

Exactly the calls PLAN named, from private `Pane` in `tui_cmd`:

```
let ChatStack { tools, notices } = pane.tools(&config, vault, confirm, diagnostics);
pane.spawner().spawn(/* Session::turn */);
```

`writes()` is not entered around the turn. `Pane::tools` already hands the pane's `WriteLane` to the tool stack (`run_derive` holds it across the derive). Entering it again around the whole turn would deadlock the single-permit mutex. The getter keeps `#[cfg_attr(not(test), allow(dead_code))]` because M12d's watcher is the other caller; M12e uses the lane through `tools()`.

`spawner()` returns a `Handle`. Spawned work cannot outlive the pane (`work_spawned_on_the_pane_cannot_outlive_it` still holds). No `Drop` on `Pane`. Field order is still memory before runtime.

`compose_session` is `pub(crate)` and remains the one production composition (`the_production_session_composition_installs_a_real_recall_injector`).

## Send

`Action::Send` (already bound) now: trim the draft; empty / whitespace is a no-op; otherwise push a person `Turn::Said`, open a Mooshik `Said` with `tui.turn_pending` (`…`), clear the composer. The live event loop then reads `App::outbound()` and `TurnDrive::start`s `Session::turn` on the pane runtime. `--demo` applies Send in the model (no silent data loss) but passes `None` for the drive, so no companion and no Memory.

## Esc

Idle: `Esc` is still `Action::Quit`. In-flight: `Action::Cancel` — `Cancellation::cancel()`, `running` stays true. The truncated assistant turn stays until `finish_turn` hears the stream stop; an empty one becomes `companion.cancelled`. A second Esc after that is Quit. `q` / `^C` still leave (they are not the cancel binding). Documented in `esc_cancels_an_in_flight_turn_and_quits_when_idle` and `esc_while_in_flight_does_not_quit`.

## Streaming

`on_token` is required and sends into an `mpsc` channel. The loop drains every pass, not only on tick timeout. Poll is `STREAM` while `turn_in_flight()`, `TICK` when idle. A partial `Said` survives `App::refresh` because conversation is already `mem::take`n across rebuilds.

## Failure

`CompanionError` is classified through its `Display` (`en.toml` keys). A 404 becomes that sentence as the assistant turn. Not a panic, not silence. Driven against `MockServer` (`a_failed_session_turn_becomes_a_turn`) with no Vertex / Cloud SQL.

## Notices

Assembly-time `ChatStack.notices` are Mooshik turns at session start. Execute-time diagnostics (permissions panic, memory-tool failure, MCP spawn / dispatch) go through `Diagnostics`. CLI default is stderr; the pane installs a channel the loop drains into `App::note`. `executor_for_chat` still `eprintln!`s its two assembly notices (`the_cli_still_prints_its_notices_to_stderr`).

## Confirm

**Deliberate choice: deny the prompt class on the pane path.** `converse` passes `Box::new(|_| false)`. A Confirm that reads stdin hangs the pane; making approval a `Turn::Cautioned` (artboard `1d`) is a bigger shape than this milestone's contract. The gate still *asks* the caller — `the_pane_path_asks_the_caller_rather_than_stdin` stays meaningful. Scratch's inner stdin prompt remains shut by `ScratchConfig::always_confirmed()` via `MemoryTools::chat_scratch`.

## Tests added

1. `sending_moves_the_draft_into_a_user_turn_and_shows_pending` (replaces `sending_does_not_discard_the_draft`)
2. `an_empty_draft_on_enter_sends_nothing`
3. `tokens_append_and_a_refresh_does_not_drop_a_partial_turn`
4. `a_failed_companion_error_becomes_a_turn` + `a_failed_session_turn_becomes_a_turn` (MockServer)
5. `esc_while_in_flight_does_not_quit` + `esc_cancels_an_in_flight_turn_and_quits_when_idle`
6. `pub(crate) fn compose_session` assertion on the existing composition pin
7. `the_live_path_wires_send_to_session_turn`
8. `the_pane_turn_path_does_not_print` + event-loop / mcp_host / gate source pins
9. existing `demo_opens_no_database_and_never_reaches_the_pane` unchanged
10. file-size: no file over 1000

## Left out, with why

- Artboards `1e` / `1f` / `1g` — out of scope.
- Approval as a `Turn::Cautioned` — see Confirm. Acceptable per PLAN; 1d is the better answer if it is taken later.
- `pane.writes().enter()` around the turn itself — would deadlock the lane the tool stack already holds.
- Making `Pane` `pub` — forbidden.
- M12d watcher, M12f artifacts, PLAN status table, push.

## Checks

- `cargo fmt`
- `env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL cargo test --locked` — 592 lib + 1 integration passed, 2 ignored
- `cargo clippy --all-targets --locked -- -D warnings` — clean
