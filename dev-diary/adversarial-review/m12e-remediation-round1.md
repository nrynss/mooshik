# M12e round-1 remediation

Remediates all six findings in `m12e-round1.md` (1 × P1, 4 × P2, 1 × P3).
No deferrals. Branch `m12e-pane-converses`, nothing pushed. All runs in a
clean env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`).

Gates after the fix: `cargo test --locked` → **597 lib passed, 0 failed, 2
ignored** (pre-existing: `memory::ops` live-Cloud, `tui::screen::tests::eyeball`)
**+ 1 integration** (`tests/report_pin.rs`). Five new pins on top of the
review's 592. `cargo fmt --check` clean. `cargo clippy --all-targets --locked
-- -D warnings` clean. No file at or over 1000 (`tui/mod.rs` 934,
`tui_cmd.rs` 854, `tools/tests.rs` 905).

---

## R1 (P1) — second Enter while in flight starts another turn and clobbers cancel

**What was wrong.** `send_draft` returned early when `in_flight` was `Some`,
but `outbound()` stayed `Some` for the whole flight, so the live Send arm
called `drive.start` again. `PaneTurn::start` replaced `self.cancel` without
cancelling the previous handle. Esc then signalled the second turn; the first
kept running.

**What changed.**
- `src/tui/mod.rs` — the Send arm remembers `let opened = !app.turn_in_flight()`
  before `apply` and only `start`s when `opened`. `outbound()` is no longer
  treated as a one-shot.
- `src/cli/tui_cmd.rs` — `PaneTurn::start` returns if `self.cancel.is_some()`,
  so a live handle is never replaced.
- `src/tui/app.rs` — `outbound` docs now say it is Some for the whole flight.

**New pins.**
- `a_second_enter_does_not_spawn` (`src/tui/mod.rs`) — source pin: `let opened
  = !app.turn_in_flight()` then `if opened` then `drive.start(&text)`, in that
  index order. Also requires `drive.cancel()` so Esc after one Enter still
  reaches the handle.
- `a_send_while_in_flight_is_ignored` (`src/tui/app_tests.rs`) — behavioural:
  second Send keeps the draft, keeps the first `outbound()`, adds no turns.
- `esc_after_one_enter_still_stops_the_turn_on_screen` (`src/cli/tui_cmd.rs`)
  — `start` contains `if self.cancel.is_some()` before `self.cancel = Some`,
  and the event loop still calls `drive.cancel()`.

**Re-run the surviving mutation.** Press Enter, then Enter again, then Esc:
`start` must not run the second time, and Esc must still cancel the turn on
screen. In source: drop `if opened` (or `let opened`) so `drive.start` runs
whenever `outbound()` is Some — `a_second_enter_does_not_spawn` goes red.
Delete the `if self.cancel.is_some() { return; }` guard —
`esc_after_one_enter_still_stops_the_turn_on_screen` goes red.

---

## R2 (P2) — wiring pin stays green if `live()` never calls `converse`

**What was wrong.** `the_live_path_wires_send_to_session_turn` searched
`converse` / `PaneTurn` bodies, so replacing `converse(...)` with
`draw(..., None)` in `live()` left every string the pin used, with `converse`
unused.

**What changed.** The same pin now splits the `live` body between `fn live(`
and `fn converse(` and asserts it contains `converse(` and does not contain
`draw(`.

**New pin.** None — the existing `the_live_path_wires_send_to_session_turn`
was tightened.

**Re-run mutation C.** In `src/cli/tui_cmd.rs` `live()`, replace
`converse(...)` with `draw(..., None)`. The pin fails: "live must call
converse, not skip it" / "live must not pass None as the turn drive by drawing
directly".

---

## R3 (P2) — drain-only-on-timeout stays green

**What was wrong.** Moving `drive.drain` into the `if !event::poll(wait)?`
branch kept every `contains` the pin used. Tokens waited for a quiet poll; a
key skipped drain.

**What changed.** `the_event_loop_drains_every_pass_and_shortens_the_poll_in_flight`
now requires `drive.drain(&mut app)` at a smaller index than `event::poll(wait)`
in the loop body.

**New pin.** None — the existing drain pin was tightened.

**Re-run mutation D.** Move `drive.drain` into the `if !event::poll(wait)?`
arm only. The pin fails: "drain must run before poll so a key does not skip
tokens".

---

## R4 (P2) — `lambo_err` `eprintln!` stays green

**What was wrong.** `the_pane_turn_path_does_not_print` only reads
`live`/`converse`/`PaneTurn`. Replacing `diagnostics.emit` with `eprintln!`
in `lambo_err` (PLAN's named MemoryTools execute-time site) left every print
pin green and corrupted the alternate screen on a failed derive.

**What changed.** New source pin on `MemoryTools`' production half:
`lambo_err`, `lambo_run_err`, and `execute` must not contain
`eprintln!`/`print!`/`eprint!`, and must contain `diagnostics.emit`
(whitespace-stripped, because rustfmt splits `self.diagnostics` / `.emit`).

**New pin.** `execute_time_failures_go_through_diagnostics_not_print`
(`src/tools/tests.rs`).

**Re-run mutation H.** In `lambo_err`, replace `self.diagnostics.emit(...)`
with `eprintln!(...)`. The pin fails: `lambo_err must not eprintln!`.

---

## R5 (P2) — deleting `chat_scratch` stays green (inner stdin hang)

**What was wrong.** `MemoryTools::over` defaults to `ScratchConfig::default()`
(stdin confirm). On `scratch = 'allow'` the gate never asks, so the inner
prompt is the one that runs. PLAN named `chat_scratch` as what holds this
shut. Dropping `.with_scratch(MemoryTools::chat_scratch(config))` left the
existing pins green.

**What changed.**
- `the_over_an_open_handle_factory_never_prints_and_never_opens` now requires
  `with_scratch(MemoryTools::chat_scratch` in the `executor_over_memory` body.
- New behavioural pin with `scratch = 'allow'`: executing `run_scratch_script`
  must run (JSON `exit_code` 0, stdout contains `hi`), not fall through to
  stdin (`tools.scratch_denied` or a hang).

**New pin.** `the_pane_path_holds_the_inner_scratch_confirm_shut`
(`src/tools/tests.rs`). Source half lives on the existing factory pin.

**Re-run mutation I.** Drop `.with_scratch(MemoryTools::chat_scratch(config))`
from `executor_over_memory`. The factory pin fails on
`with_scratch(MemoryTools::chat_scratch`. The behavioural pin fails with
`inner confirm must not be stdin` (EOF → `scratch_denied`, not JSON).

---

## R6 (P3) — keymap still promises Esc leaves

**What was wrong.** In-flight Esc is Cancel and does not leave. The module
header, `tui.after_help`, and both bottom-rule hints still said `Esc leave` /
`Esc or ^C leaves`. Idle Esc is still Quit.

**What changed.**
- `src/tui/input.rs` header — `Esc` stops a reply then leaves; `q` and `^C`
  leave. Idle Esc stays Quit (the arm is unchanged).
- `tui.after_help` — `Esc stops a reply then leaves · ^C leaves`.
- `hint_today` / `hint_week` / `hint_narrow` — `Esc leave` → `Esc stops`
  (same length, so the 47-character week rule and the Today health rule keep
  their artboard layout). Header and `--help` carry the stop-then-leave
  wording; the hints cannot grow without dropping the week scope at 100
  columns.
- `src/tui/screen/chrome.rs` layout fixture now reads `tui.hint_today`.
- `src/tui/screen/narrow.rs` asserts `Esc stops`.

**New pin.** None — the strings are the pin. Hint tests go through
`text::get`.

**Re-run.** Put `Esc leave` back in `hint_today`. The hint is a promise the
keymap does not keep for an in-flight Esc. The header and `after_help` must
not say in-flight Esc leaves.

---

## Mutations C / D / H / I now fail

| # | Mutation | Pin that now fails |
|---|---|---|
| C | `live()`: `converse(...)` → `draw(..., None)` | `the_live_path_wires_send_to_session_turn` |
| D | `drive.drain` only inside the poll timeout arm | `the_event_loop_drains_every_pass_and_shortens_the_poll_in_flight` |
| H | `lambo_err`: `diagnostics.emit` → `eprintln!` | `execute_time_failures_go_through_diagnostics_not_print` |
| I | drop `.with_scratch(MemoryTools::chat_scratch(config))` | factory pin + `the_pane_path_holds_the_inner_scratch_confirm_shut` |
