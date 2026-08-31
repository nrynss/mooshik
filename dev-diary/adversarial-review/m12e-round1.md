# M12e round 1 — adversarial review of the pane converses

Reviewed against HEAD `49a504d` (`49a504d2ae0d5a835cb12eb9bb2a57fc37e80d01`),
branch `m12e-pane-converses`, tree **clean** at start (implementation already
committed). Scope: the live pane turn path (`src/cli/tui_cmd.rs` `live` /
`converse` / `PaneTurn`), the `TurnDrive` seam and event loop (`src/tui/mod.rs`),
`App::{send,cancel,token,finish,refresh}` (`src/tui/app.rs`, `app_tests.rs`,
`input.rs`), execute-time `Diagnostics` (`src/tools/{diagnostics,mod,permissions}.rs`,
`src/mcp_host/mod.rs`), `compose_session` visibility, and the M12e pins. Authority:
`docs/SPEC.md`, `dev-diary/PLAN.md` **M12e — what to move, and what not to break**,
implementation report `dev-diary/adversarial-review/m12e-implementation.md`.

Every mutation below was made against a byte copy, run, restored, and
`sha256sum`-verified identical to the pre-mutation state. `git status --porcelain`
after this round shows **only this review record**. Nothing committed.

The M12c round-1 failure mode — report claiming CLI wiring and view surfacing
that were not in the tree — **does not reproduce here**. `live()` calls
`converse()`, `converse()` calls `compose_session` and `pane.tools` /
`pane.spawner()`, `PaneTurn::start` spawns `Session::turn`, `Action::Send` is
no longer empty, `compose_session` is `pub(crate)`. The hole is smaller, and
it is in the live Send arm, not in a missing function.

## Verdict

**REMEDIATE** — 1 × P1, 4 × P2, 1 × P3.

The milestone is mostly present: a question on the live path does spawn
`Session::turn` on the pane runtime, tokens drain into the pane, idle `Esc`
still quits, in-flight `Esc` is `Cancel`, `--demo` still never opens Memory,
the lease is claimed once, stdin is not reached on the shipped pane path, and
the WriteLane is held inside `run_derive` rather than around the whole turn.
What does not hold is the Send/Esc contract once Enter is pressed a second
time while a turn is already on screen, and four source pins that stay green
with their named hazard present.

---

## What the report claims vs. what the tree shows

| Report claim | Tree |
|---|---|
| `live()` / `converse()` compose over the pane, spawn `Session::turn`, drain tokens | **TRUE.** `live:245` calls `converse`; `converse:273-277` builds `pane.tools` + `compose_session` + `PaneTurn::new(pane.spawner(), …)`; `PaneTurn::start:361-369` `spawn`s `session.turn`. |
| `compose_session` is `pub(crate)` | **TRUE.** `companion/chat.rs:124` and re-export `companion/mod.rs:24`. |
| `--demo` never opens Memory | **TRUE.** `tui:215` draws a fixture with `None, None`. Pin still bites (mutation G). |
| Lease claimed once; no `executor_for_chat` / `MemoryTools::for_chat` / second `memory::open` | **TRUE.** `Pane::open` is the one `crate::memory::open`; `Pane::tools` goes through `executor_over_memory`. Pane stays private. |
| Confirm is `Box::new(\|_| false)`; scratch inner prompt held by `chat_scratch` | **TRUE on the shipped tree.** Gate never falls through to stdin. Deleting `with_scratch` is unpinned (P2-4). |
| `writes()` is not entered around the turn; `run_derive` holds the lane | **TRUE.** `tools/mod.rs:399-400` `let _lane = writes.enter().await` then `memory.derive`. `PaneTurn::start` does not enter. Wrapping the whole turn *and* `run_derive` would deadlock the single-permit mutex; they did not. `writes()` keeps `allow(dead_code)` for M12d; the comment does not lie. |
| Execute-time prints go through `Diagnostics`; CLI `executor_for_chat` still `eprintln!`s | **TRUE for the named sites** (permissions panic, `lambo_err` / panic, eight mcp_host sites). CLI pin still requires the two assembly `eprintln!`s. A pane-path `eprintln!` put back in `converse` turns `the_pane_turn_path_does_not_print` red; the same hazard in `lambo_err` does not (P2-3). |
| Drain every pass; poll `STREAM` in flight, `TICK` idle | **TRUE in source.** Drain-only-on-timeout leaves the pin green (P2-2). Poll-always-`TICK` (`let wait = TICK`) turns it red. |
| Failed turn: MockServer drives `Session::turn` | **TRUE.** `a_failed_session_turn_becomes_a_turn` calls `session.turn` against `MockServer::error(404)`, then `finish_turn(Failed(error.to_string()))`. It does **not** drive `PaneTurn::drain`; classification in the `Finished` arm is unpinned, but the error is not silent and does not panic. |
| File sizes under 1000 | **TRUE.** `tui/mod.rs` 890, `tui_cmd.rs` 791, `tools/mod.rs` 762, `app.rs` 471. `view.rs` 992 is unchanged by this commit. |
| `Action::Send` moves the draft; empty is a no-op; in-flight Send is ignored | **HALF.** The model (`send_draft:277-278`) returns early. The event loop still `drive.start`s whenever `outbound()` is `Some` — which it is for the whole flight (P1). |

---

## What held up under attack

* **The M12c hole is closed.** `compose_session` is crate-visible, `converse`
  is the one production composition, `Action::Send` is no longer an empty
  handler, `--demo` still never reaches `Pane::open`. Replacing
  `compose_session(...)` in `converse` with an inline `Session::new`…
  `.with_executor`…`.with_recall` turns `the_live_path_wires_send_to_session_turn`
  red. Deleting `self.spawner.spawn` from `PaneTurn::start` does the same.
* **`--demo` still cannot open Memory.** Pointing the demo arm at `live(layout)`
  fails `demo_opens_no_database_and_never_reaches_the_pane` on the
  `draw(crate::tui::demo(` assert, verbatim.
* **Lease is claimed once.** Production `tui_cmd` contains exactly one
  `crate::memory::open(`, no `executor_for_chat(`, no `MemoryTools::for_chat(`.
  `Pane` is module-private. Field order is still memory before runtime; no
  `Drop` impl.
* **Stdin is not reached on the shipped pane path.** `converse` installs
  `Box::new(|_| false)` and never names `interactive_confirm` / `std::io::stdin`.
  `compose_chat_stack(…, Some(confirm), …)` uses `with_confirm`, not
  `GatedTools::new`'s default. `executor_over_memory` still sets
  `MemoryTools::chat_scratch` (`always_confirmed()`). `eprint!` in
  `permissions::interactive_confirm` and `scratch::interactive_confirm` is
  unreachable from this path. PLAN's "deny the prompt class" choice is what
  shipped; 1d was not required.
* **WriteLane is not double-entered and is not skipped on derive.**
  `Pane::tools` clones `self.writes` into `MemoryTools::over`. `run_derive`
  enters inside the worker's `block_on`, across the `derive` await. Recall
  injection goes through the same `ToolExecutor` (`lambo_recall` only). MCP
  does not write the graph. Wrapping `Session::turn` in `writes().enter()`
  would deadlock with `run_derive`; they did not.
* **Spawned work cannot outlive the pane.** `spawner()` returns a `Handle`.
  `work_spawned_on_the_pane_cannot_outlive_it` still holds. No `block_on` of
  the turn on the event-loop thread (`!drive.contains("block_on")`).
* **Streaming / Esc / failure / pending, on the model.** Tokens append;
  `App::refresh` `mem::take`s the conversation so a partial `Said` survives.
  Empty / whitespace draft is a no-op. Idle `Esc` is `Quit`; in-flight `Esc`
  is `Cancel` and does not set `running = false`; a second `Esc` after
  `finish_turn` is `Quit`. `q` / `^C` still leave. Cancelled-with-tokens keeps
  the truncated body; cancelled-empty becomes `companion.cancelled`. A 404
  from `MockServer` becomes `companion.http_status` as the assistant turn.
* **CLI still prints.** `the_cli_still_prints_its_notices_to_stderr` requires
  both assembly `eprintln!`s in `executor_for_chat`.
* **`eprintln!` in `converse` is caught.** Pin goes red (mutation F).
* **Poll-always-`TICK` is caught** in the form `let wait = TICK` (mutation E),
  because that drop also drops `app.turn_in_flight()` / `STREAM` from the
  loop body.
* **fmt / clippy / file cap.** Clean. No file over 1000.

---

## Findings

| # | Priority | File | Finding + remediation |
|---|---|---|---|
| **R1** | **P1** | `src/tui/mod.rs:414-419`, `src/tui/app.rs:209-218`, `src/cli/tui_cmd.rs:355-357` | **A second Enter while a turn is in flight starts another `Session::turn` and clobbers the cancel handle.** `send_draft` returns early when `in_flight` is `Some` (draft kept — the comment is right about the *model*). `outbound()` is documented as "if Send just opened one" but returns `Some` for the whole flight. The live match arm then `drive.start`s whenever `outbound()` is `Some`. `PaneTurn::start` replaces `self.cancel` without cancelling the previous handle. After a double Enter: the first turn is the one on screen; `Esc` signals only the second `Cancellation` (the first keeps running); the second task queues on the session mutex, then its tokens/`Finished` hit a cleared `in_flight` and vanish; the same question is sent to the companion twice and can derive twice. That is the load-bearing Esc contract unmet, plus a silent extra write. **Remediation:** only `start` when this Send actually opened a flight (e.g. remember `!app.turn_in_flight()` before `apply`, or have `send_draft` return `bool`, or take a one-shot outbound). `start` must not replace a live cancel handle. Pin both: second Enter does not spawn; `Esc` after one Enter still stops the turn on screen. |
| **R2** | **P2** | `src/cli/tui_cmd.rs:667-715` (`the_live_path_wires_send_to_session_turn`) | **The wiring pin stays green if `live()` never calls `converse`.** Replacing `converse(...)` with `draw(..., None)` — the pre-M12e live path, `TurnDrive` absent — leaves every string the pin searches for in the now-dead `converse` / `PaneTurn` bodies. Three runs, three passes, with `converse` reported unused. Round-2 precedent: a pin passing with the hazard present is a P2. The mutations that *delete* `compose_session(` from `converse` or `self.spawner.spawn` from `start` *do* go red; the pin cannot see that `live()` is the only caller. **Remediation:** assert the `live` body (the split between `fn live(` and `fn converse(`) contains `converse(` and does not pass `None` as the turn drive. |
| **R3** | **P2** | `src/tui/mod.rs:394-406` and `499-530` (`the_event_loop_drains_every_pass_and_shortens_the_poll_in_flight`) | **Drain-only-on-timeout stays green.** Moving `drive.drain` from the top of the loop into the `if !event::poll(wait)?` branch — tokens wait for a quiet poll, and a key skips drain entirely — keeps every `contains` the pin uses (`drive.drain(&mut app)`, `STREAM`, `TICK`, `turn_in_flight`, `drive.start`, `drive.cancel`). The pin's own comment names this hazard and does not catch it. Poll-always-`TICK` as `let wait = TICK` *is* caught (it also drops `turn_in_flight()` / `STREAM`). **Remediation:** require drain *before* `event::poll` in the loop body (index order), so a drain that lives only inside the timeout arm fails. |
| **R4** | **P2** | `src/tools/mod.rs:498-517` (`lambo_err` / `lambo_run_err`) | **A pane-path `eprintln!` put back at PLAN's named MemoryTools execute-time sites does not turn any print pin red.** `the_pane_turn_path_does_not_print` only reads `live`/`converse`/`PaneTurn`; `the_over_an_open_handle_factory_never_prints` only reads `executor_over_memory`; mcp_host and the gate have their own pins. Replacing `self.diagnostics.emit(...)` in `lambo_err` with `eprintln!` — the pre-M12e form, which corrupts the alternate screen on a failed derive — left all six of those pins green. **Remediation:** a source pin on `MemoryTools`' production half forbidding `eprintln!`/`print!`/`eprint!` in `lambo_err` / `lambo_run_err` / `execute`, and requiring `diagnostics.emit`. |
| **R5** | **P2** | `src/tools/mod.rs:634-638` and `src/tools/tests.rs:737-765` | **Deleting `with_scratch(MemoryTools::chat_scratch(config))` stays green, and that is the inner stdin hang.** `MemoryTools::over` defaults to `ScratchConfig::default()`, whose confirm is `interactive_confirm` (reads stdin, `eprint!`s the prompt). On `[permissions] scratch = 'allow'` the gate never asks, so the inner prompt is the one that runs. PLAN named `chat_scratch` as the thing that holds this shut. `the_pane_path_asks_the_caller_rather_than_stdin` uses `scratch = 'prompt'` and is refused at the gate before the inner executor; `the_over_an_open_handle_factory_never_prints` does not mention `chat_scratch`; `the_live_path_wires_send_to_session_turn` only forbids `interactive_confirm` *inside `converse`*. **Remediation:** require `with_scratch(MemoryTools::chat_scratch` in the `executor_over_memory` body. A behavioral pin with `scratch = 'allow'` that asserts the inner confirm is not stdin is stronger if it fits. |
| **R6** | **P3** | `src/tui/input.rs:3-5`, `src/text/en.toml:223`, `:275-276` | **The keymap still promises that `Esc` leaves, in the module header, `--help`, and both bottom-rule hints.** `input.rs` claims to list everything bound "in full" and that the list and the hints are "the same list, deliberately" — "a hint that does nothing is worse than no hint". In-flight `Esc` is now `Cancel` and does not leave; the user-visible strings are still `Esc leave` / `Esc or ^C leaves`. The arm itself is documented correctly (`input.rs:104-108`). **Remediation:** update the header, `tui.after_help`, `hint_today` and `hint_week` so they do not promise leave for an in-flight `Esc` (a second hint while `in_flight`, or wording that covers stop-then-leave). |

---

## Mutation table

Copies at `/tmp/m12e-review-orig/`. Restored and `sha256sum`-identical after
each row.

| # | Mutation | Pin | Result |
|---|---|---|---|
| A | `converse`: `compose_session(...)` → inline `Session::new`…`.with_recall` | `the_live_path_wires_send_to_session_turn` | **caught** — "the live path must compose through compose_session" |
| B | `PaneTurn::start`: `self.spawner.spawn` deleted | `the_live_path_wires_send_to_session_turn` | **caught** — "start must spawn on the pane handle" |
| C | `live()`: `converse(...)` → `draw(..., None)` (dead `converse` / `PaneTurn` still in the file) | `the_live_path_wires_send_to_session_turn` | **SURVIVES** — pass, with `converse` unused (P2, R2) |
| D | `event_loop`: `drive.drain` moved inside the `poll` timeout arm only | `the_event_loop_drains_every_pass_and_shortens_the_poll_in_flight` | **SURVIVES** — pass (P2, R3) |
| E | `let wait = TICK` (poll always idle) | same | **caught** — "the poll must know whether a turn is in flight" |
| F | `eprintln!("assembling the pane turn")` in `converse` | `the_pane_turn_path_does_not_print` | **caught** — "the pane turn path must not eprintln!" |
| G | demo arm → `live(layout)` | `demo_opens_no_database_and_never_reaches_the_pane` | **caught** — "the demo arm must draw a fixed workspace directly" |
| H | `lambo_err`: `diagnostics.emit` → `eprintln!` | `the_pane_turn_path_does_not_print`, `the_over_an_open_handle_factory_never_prints`, mcp_host / gate print pins, derive-lane pin | **SURVIVES** — all six green (P2, R4) |
| I | `executor_over_memory`: drop `.with_scratch(MemoryTools::chat_scratch(config))` | `the_over_an_open_handle_factory_never_prints`, `the_pane_path_asks_the_caller_rather_than_stdin`, `the_live_path_wires_send_to_session_turn` | **SURVIVES** — all three green (P2, R5) |

Pre-mutation hashes, restored after every row:

* `src/tui/mod.rs` `4fc2bcd576c75fa7f8e064bb5458fe4d8fbfcb94e203f6d5f9aa3e7afcf85dca`
* `src/cli/tui_cmd.rs` `6af9b13293b539a3530b97a91987ff71cacd3bc626ad6a9c3d22d116106983e8`
* `src/tools/mod.rs` `1ee6a522a6d084e62816b1be3c8d9352cc2600fb4d8179ad938d4596705fd5d4`

---

## Gates

Run by me, clean env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`):

* `cargo test --locked` → **592 lib passed, 0 failed, 2 ignored** (pre-existing:
  `memory::ops` live-Cloud, `tui::screen::tests::eyeball`) **+ 1 integration
  passed** (`tests/report_pin.rs`) **+ 0 doc**. Matches the implementation
  report's 592 + 1 / 2 ignored.
* `cargo fmt --check` → clean (exit 0).
* `cargo clippy --all-targets --locked -- -D warnings` → clean (exit 0).
* File-size cap → clean. Nothing at or over 1000. Soft-600 files that this
  commit grew (`tui/mod.rs` 890, `tui_cmd.rs` 791) stay under the CI cap.

Ambient `LAMBO_POSTGRES_DSN` is set in this shell; every `cargo` invocation
ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

---

## What was executed vs. only read

**Executed.** Mutations A–I, each reverted and hash-verified, with the named
pin runs. The full locked suite, fmt, clippy. File-size `wc -l`. HEAD / branch
/ porcelain before and after.

**Read, not executed.** A live `mooshik tui` against a real companion (no
controlling terminal in this review, and the MockServer test is the 404
path). The reversed-order WriteLane deadlock (source shows `start` does not
enter; `run_derive` does). Non-unix stubs. Artboards `1e`/`1f`/`1g` and M12d,
out of scope.

---

## Notes for the next round

* R1 is the whole of the user-facing contract that is actually broken. Fixing
  the Send arm (and pinning a second Enter) is enough for Esc-stops-the-turn
  to hold under a mashed Enter; `start` replacing `cancel` is the same hole
  seen from the other end.
* R2–R5 are pin quality, in the M12b-R1-1 shape: the behaviour on the shipped
  tree is right, the pin does not see the plausible regression. Round 2 should
  not APPROVE while any of the four mutations above still pass.
* `writes()` `allow(dead_code)` is not a finding. M12d is the other caller;
  M12e uses the lane through `tools()`.
* PLAN's "classified through `cli::failure`" is not what shipped —
  `CompanionError`'s `Display` / `en.toml` is. Spec does not require the CLI
  classifier. Not a finding; the 404 test is honest about which type it uses.

## Explicit

`git status --porcelain` after this round shows only
`dev-diary/adversarial-review/m12e-round1.md`. All mutations reverted.
Checksums above. Nothing committed.
