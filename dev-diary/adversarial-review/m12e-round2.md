# M12e round 2 — adversarial re-review of the round-1 remediation

Reviewed against HEAD `d5c5909` (`d5c59090931be67efb5a606b03941da58b23d659`),
branch `m12e-pane-converses`, tree **clean** at start (remediation already
committed). Scope: the six round-1 findings (R1–R6) and the pins/strings
claimed in `dev-diary/adversarial-review/m12e-remediation-round1.md`. Authority
unchanged: `docs/SPEC.md`, `dev-diary/PLAN.md` **M12e**, the implementation
report, and round 1.

Round 1 said: "Round 2 should not APPROVE while any of the four mutations
above still pass." C, D, H, and I were re-executed. All four now fail the
named pins. The two R1 attacks also go red.

Every mutation was made against a byte copy, run, restored, and
`sha256sum`-verified identical to the pre-mutation state.
`git status --porcelain` after this round shows **only this review record**.
Nothing committed.

## Verdict

**APPROVE** — 0 × P1, 0 × P2, 0 × P3.

Zero residue. The P1 Send/Esc hole is gated on both sides and both gates
have pins that bite the round-1 regression. The four source pins that stayed
green with their hazard present now fail that same hazard. User-visible
keymap strings no longer promise leave for an in-flight `Esc`.

---

## What the remediation claims vs. what the tree shows

| Claim | Tree |
|---|---|
| Send arm: `let opened = !app.turn_in_flight()` before `apply`, `start` only when `opened` | **TRUE.** `src/tui/mod.rs:418-425`. |
| `PaneTurn::start` returns if `self.cancel.is_some()` | **TRUE.** `src/cli/tui_cmd.rs:358-360`, before `self.cancel = Some`. |
| `a_second_enter_does_not_spawn` — index order `let opened` then `if opened` then `drive.start` | **TRUE**, and it bites (R1-a). |
| `a_send_while_in_flight_is_ignored` — model keeps draft and first outbound | **TRUE.** `src/tui/app_tests.rs:330-359`. |
| `esc_after_one_enter_still_stops_the_turn_on_screen` — guard + `drive.cancel()` + `let opened` | **TRUE**, and it bites (R1-a and R1-b). |
| Wiring pin now splits `live` and forbids `draw(` | **TRUE.** Mutation C fails "live must call converse, not skip it". |
| Drain pin requires `drive.drain` index < `event::poll(wait)` | **TRUE.** Mutation D fails "drain must run before poll so a key does not skip tokens". |
| `execute_time_failures_go_through_diagnostics_not_print` on `lambo_err` / `lambo_run_err` / `execute` | **TRUE.** Mutation H fails `lambo_err must not eprintln!`. Compact `diagnostics.emit` so rustfmt splits do not hide it. |
| Factory pin requires `with_scratch(MemoryTools::chat_scratch`; behavioural pin with `scratch = 'allow'` | **TRUE.** Mutation I fails both; the behavioural pin printed `Run this script? [y/N]` and then `inner confirm must not be stdin`. |
| R6 strings | **TRUE.** Header, `after_help`, `hint_today` / `hint_week` / `hint_narrow` no longer say `Esc leave` for in-flight Esc. |
| 597 lib + 1 integration, 2 ignored; files under 1000 | **TRUE.** `tui/mod.rs` 934, `tui_cmd.rs` 854, `tools/tests.rs` 905. |

The `live` split does not drop the converse/drive checks: `compose_session(` /
`pane.tools(` / `pane.spawner()` / `self.spawner.spawn` still run against
`converse` and `PaneTurn`. `!live.contains("draw(")` does not false-red on the
live comment "a draw that failed" (no paren).

---

## What held up under attack

* **R1 — second Enter.** Dropping `let opened` / `if opened` so `drive.start`
  runs whenever `outbound()` is `Some` turns `a_second_enter_does_not_spawn`
  red ("start must be gated on Send having just opened a flight") and
  `esc_after_one_enter_still_stops_the_turn_on_screen` red on the same
  missing string. Deleting `if self.cancel.is_some() { return; }` turns
  `esc_after_one_enter_still_stops_the_turn_on_screen` red ("start must not
  replace a live cancel handle") and the wiring pin red on
  `find("if self.cancel.is_some()")`. The model pin still holds the draft
  and the first outbound. Belt and suspenders: even if the loop gate were
  skipped, `start` would refuse a live handle; even if `start` lost the
  guard, the loop would not call it on a second Enter.
* **R2 / mutation C.** `live()` → `draw(..., None)` with dead `converse` /
  `PaneTurn` still in the file now fails. Round-1 survivor is closed.
* **R3 / mutation D.** Drain moved into the `poll` timeout arm only now
  fails on index order. Round-1 survivor is closed.
* **R4 / mutation H.** `lambo_err` `eprintln!` now fails the new MemoryTools
  pin. The older print pins still do not read those functions — they do not
  have to, because this one does.
* **R5 / mutation I.** Dropping `with_scratch(MemoryTools::chat_scratch(config))`
  fails the factory pin and the `scratch = 'allow'` behavioural pin. The
  behavioural pin actually reached `scratch::interactive_confirm` (the
  prompt printed); EOF became `tools.scratch_denied`, not JSON.
* **R6.** `input.rs:5` is `Esc stops a reply then leaves`. `tui.after_help`
  is `Esc stops a reply then leaves · ^C leaves`. Hints are `Esc stops`
  (same length as `Esc leave`, so the 47-character week rule still fits).
  `chrome.rs` layout fixture reads `tui.hint_today` rather than a hardcoded
  `Esc leave`. `narrow.rs` asserts `Esc stops`. Idle `Esc` is still `Quit`;
  `q` / `^C` still leave. No user-visible string promises leave for an
  in-flight `Esc`.
* **No new residue.** File sizes under 1000. The `live` split still sees
  `converse(` as a call, not only as `fn converse(`. `esc_after_one_enter`
  looks for `if self.cancel.is_some()`, not the `cancel()` method's
  `if let Some(cancel)`, so deleting the guard cannot hide behind that
  arm. fmt / clippy clean.

---

## Findings

None.

---

## Mutation table

Copies at `/tmp/m12e-r2-orig/`. Restored and `sha256sum`-identical after each
row.

| # | Mutation | Pin | Result |
|---|---|---|---|
| C | `live()`: `converse(...)` → `draw(..., None)` | `the_live_path_wires_send_to_session_turn` | **caught** — "live must call converse, not skip it" |
| D | `drive.drain` only inside the `poll` timeout arm | `the_event_loop_drains_every_pass_and_shortens_the_poll_in_flight` | **caught** — "drain must run before poll so a key does not skip tokens" |
| H | `lambo_err`: `diagnostics.emit` → `eprintln!` | `execute_time_failures_go_through_diagnostics_not_print` | **caught** — "lambo_err must not eprintln!" |
| I | drop `.with_scratch(MemoryTools::chat_scratch(config))` | factory pin + `the_pane_path_holds_the_inner_scratch_confirm_shut` | **caught** — both fail; behavioural pin reached stdin (`Run this script? [y/N]`) |
| R1-a | drop `let opened` / `if opened` so `start` runs on `outbound()` | `a_second_enter_does_not_spawn` (and `esc_after_one_enter_…`) | **caught** — "start must be gated on Send having just opened a flight" |
| R1-b | delete `if self.cancel.is_some() { return; }` | `esc_after_one_enter_still_stops_the_turn_on_screen` (and wiring pin) | **caught** — "start must not replace a live cancel handle" |

Pre-mutation hashes, restored after every row:

* `src/tui/mod.rs` `250789316b2af030e476443d6e904e2024f3e8cbe091e490ba9e33e81edfe2d2`
* `src/cli/tui_cmd.rs` `0e02c2f9ea02ecec8db0e2d991cf5bc2366e17a6f686c9cd1a2eeab2ec2fa44e`
* `src/tools/mod.rs` `1ee6a522a6d084e62816b1be3c8d9352cc2600fb4d8179ad938d4596705fd5d4`

---

## Gates

Run by me, clean env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`):

* `cargo test --locked` → **597 lib passed, 0 failed, 2 ignored** (pre-existing:
  `memory::ops` live-Cloud, `tui::screen::tests::eyeball`) **+ 1 integration
  passed** (`tests/report_pin.rs`) **+ 0 doc**. Matches the remediation
  report (five pins on top of round 1's 592).
* `cargo fmt --check` → clean (exit 0).
* `cargo clippy --all-targets --locked -- -D warnings` → clean (exit 0).
* File-size cap → clean. `tui/mod.rs` 934, `tui_cmd.rs` 854, `tools/tests.rs`
  905, `view.rs` 992 (untouched). Nothing at or over 1000.

Ambient `LAMBO_POSTGRES_DSN` is set in this shell; every `cargo` invocation
ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

---

## What was executed vs. only read

**Executed.** Mutations C, D, H, I, R1-a, R1-b, each reverted and
hash-verified. The full locked suite, fmt, clippy. File-size `wc -l`. HEAD /
branch / porcelain before and after.

**Read, not executed.** A live `mooshik tui` against a real companion. Putting
`Esc leave` back into `hint_today` (the strings are the pin; `narrow.rs`
asserts `Esc stops` on the narrow rule, `chrome.rs` now reads `tui.hint_today`
so a revert there would still *layout*, and the user-visible copy is what R6
required). Non-unix stubs. Artboards `1e`/`1f`/`1g` and M12d, out of scope.

---

## Explicit

`git status --porcelain` after this round shows only
`dev-diary/adversarial-review/m12e-round2.md`. All mutations reverted.
Checksums above. Nothing committed.
