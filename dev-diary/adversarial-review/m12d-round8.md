# M12d round 8 — adversarial re-verification of the round-7 remediations

Reviewed against HEAD `14135e0`, branch `m12d-watcher`, tree dirty with the
round-6 and round-7 remediations (no commits). Scope: the two findings in
`m12d-remediation-round7.md` (M12d-R7-1 both cancel returns including the
file walk; M12d-R7-2 README fail-closed conjunct), the named pins those
remediations added or extended, and same-class residue (a third cancel
return, a hollow `snapshot_retaining_failed_discoveries`, a CollectCancel
probe that never hits the second load, a source pin gamed by comments,
other `available without` phrasings, a live path that still starts the TUI
without the watcher, production `AtomicBool` vs the test probe). All runs
in a clean env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u
DATABASE_URL`). Every mutation below was transient and reverted: the
touched file was restored from a byte copy taken immediately before the
mutation and `sha256sum`-verified identical afterwards (`src/cli/watcher.rs`
`40ed69cd2fb07467de2a3dd418e8d06822ca240d2a19620aa18793c116dedc5b`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`,
`src/cli/tui_cmd.rs`
`bfa0dafc1c99e08092661bdfe24c1376e6039d9f78ab268d4f0cbfc92a65f98a`).
`git status --porcelain` at the end shows the same dirty set as at the
start (`README.md`, `dev-diary/PLAN.md`, `src/cli/watcher.rs`, untracked
`m12d-remediation-round6.md`, `m12d-remediation-round7.md`,
`m12d-round7.md`) plus this record. Nothing committed by this round.
`src/tui/` was not opened.

## Verdict

**REMEDIATE** — R7-1's two production cancel returns and R7-2's claimed
README/start/`live` conjuncts are real and the named pins bite the
mutations the remediation named. Residue of the same class remains: the
new `CollectCancel` trait lets production `AtomicBool` diverge from the
file-walk probe with every named pin green (F1), and the README reject is
still an exact substring, so a one-article or punctuation restatement of
the historical lie stays green (F2). 2×P3.

## What held

- **R7-1 file-walk cancel.** Restoring only the `for (path, state)` return
  to `previous.clone()` (first cancel left on the helper) fails
  `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository` at
  `left: None right: Some(Unknown)` with "file-walk cancel must not drop a
  failed discovery that already ran" (loads == 2) and
  `failed_git_discovery_does_not_baseline_away_an_unknown_head` at
  `cancel return 1 must retain Unknown markers from git_failures, not
  return previous.clone()`. The six named R6-1 pins that ignored this
  site still pass, as round 7 showed. There is no third
  `return ChangeResult` in `collect_changes_with_cancel`.
- **R7-1 first cancel still pinned.** Restoring only the first
  `cancelled.load` return to `previous.clone()` fails
  `cancelled_collect_keeps_unknown_for_a_late_failed_repository` and
  `cancelled_late_failed_repository_replays_first_commit_after_recovery`
  (`None` vs `Some(Unknown)`) and the source pin at `cancel return 0`.
  The file-walk behavioural pin stays green, so the two sites are not
  one vacuous check.
- **R7-1 helper body.** Emptying `snapshot_retaining_failed_discoveries`
  to a bare `previous.clone()` (call sites left in place) fails both
  cancelled behavioural pins and the file-walk pin; the source pin stays
  green (it only requires the call name). Same documented limit as
  rounds 6–7, not a numbered finding: the production mixup is still
  caught by behaviour.
- **R7-2 claimed README mutation.** Adding `The pane remains available
  without the watcher` while keeping `fails closed at TUI startup` and
  `The watcher stops with the pane` fails
  `live_watching_fails_closed_at_tui_startup` at `README must not claim
  the pane runs without the watcher`.
- **R7-2 `pane.close()`.** Deleting `pane.close()` from the
  `Watcher::start` `Err` arm fails the same pin at `Watcher::start
  failure must close the pane`. `live` still
  `return Err(anyhow::Error::new(error))` after close; `start`'s
  `#[cfg(not(unix))]` arm still returns `WatchError::WorkspaceUnavailable`.
  The demo `--scene` path is not live watching.

## Findings

| # | Pri | File | Finding |
|---|---|---|---|
| F1 | P3 | `src/cli/watcher.rs` `impl CollectCancel for AtomicBool` (~472–479) | **The CollectCancel abstraction is a new hole: production `AtomicBool` can ignore the flag while the file-walk probe still bites.** Executed: `CollectCancel::load` for `AtomicBool` always returns `false` (the real load is evaluated and discarded). Result: all 34 watcher lib tests **pass**, including `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository` (loads == 2 on `CancelAfterFirstLoad`), `cancelled_collect_keeps_unknown_for_a_late_failed_repository` (Unknown is already on `next` from the `git_failures` loop, so a no-op cancel still looks retained), and the source pin (it only reads the two `if cancelled.load(Ordering::Acquire)` call sites). A second executed stub, `git_cancel_flag` returning a static `AtomicBool::new(false)`, is the same: 34 **pass**. Pane-stop during collect would no longer abort the file walk or git subprocess via the production impl, and no named R7-1 pin notices. **Fix:** a behavioural pin that passes `&AtomicBool::new(true)` *and* has a file in `current.files` so a no-op `load` cannot hide behind the `git_failures` loop; pin `impl CollectCancel for AtomicBool` so `load` forwards to `AtomicBool::load` and `git_cancel_flag` returns `self`. |
| F2 | P3 | `src/cli/watcher.rs` `live_watching_fails_closed_at_tui_startup` README conjunct | **The README reject is still an exact substring `available without the watcher`; a restated false claim stays green.** Executed: README keeps the true fail-closed sentences and adds `The pane remains available without a watcher` (`the` → `a`) — pin **passes**. Same with `The pane remains available, without the watcher` (comma). The claimed pane-respell (`available without the watcher`) does bite; the next one-word/punctuation dodge does not. **Fix:** reject a looser pattern (`available without` + `watcher`, or any `without the watcher` / `without a watcher` availability claim) rather than one exact five-word phrase. |

## Mutation table

All runs in the clean env. Every mutation reverted and `sha256sum`-verified
byte-identical after its run.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | file-loop cancel return → `previous.clone()` (first cancel left on the helper) | `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository`; `failed_git_discovery_does_not_baseline_away_an_unknown_head` | **both fail — bites** (`None` vs `Some(Unknown)`, loads == 2; source pin `cancel return 1`). Old R6-1 pins stay green |
| 2 | first cancel return → `previous.clone()` (file-walk left on the helper) | `cancelled_collect_keeps_unknown_for_a_late_failed_repository`; `cancelled_late_failed_repository_replays_first_commit_after_recovery`; source pin | **all three fail — bites** (`cancel return 0`). File-walk pin stays green |
| 3 | README adds `The pane remains available without the watcher` (true sentences kept) | `live_watching_fails_closed_at_tui_startup` | **fails — bites** (`README must not claim the pane runs without the watcher`) |
| 4 | `live` `Err` arm drops `pane.close()` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** (`Watcher::start failure must close the pane`) |
| 5 | `snapshot_retaining_failed_discoveries` body → `previous.clone()` (call sites kept) | cancelled behavioural pins; file-walk pin; source pin | **behavioural pins fail — bites the helper; source pin passes** |
| 6 | `impl CollectCancel for AtomicBool`: `load` always `false` | all named R7-1 pins; full watcher suite | **all pass — gap (F1)** |
| 7 | `git_cancel_flag` returns a static never-set `AtomicBool` | full watcher suite | **all pass — same gap (F1)** |
| 8 | README `The pane remains available without a watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — gap (F2)** |
| 9 | README `The pane remains available, without the watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — gap (F2)** |

## Gates (run by me, clean env)

- **`cargo test --locked --lib cli::watcher`** — **34 passed, 0 failed, 0
  ignored**, exit 0, on the restored tree after the last revert (rebuild
  after `include_str!` mutations so README/`tui_cmd.rs` were not stale).
- **`cargo fmt --check`** — exit 0.
- **`cargo clippy --locked --all-targets --all-features -- -D warnings`**
  — exit 0, zero warnings.
- **File-size:** `watcher.rs` 2531 lines — dedicated M12d module over the
  1000-line cap used elsewhere; not split this round.

## Executed vs read

**Executed:** the nine mutations above (each reverted and hash-verified);
the named pins on the clean tree and after each mutation; the watcher
lib filter (34) on the restored tree and on the CollectCancel stubs.
Non-Unix `Watcher::start` is source-pinned, not run on this Linux host.

**Read (not re-executed):** PLAN.md §M12d (failed discovery including a
repo that appears after the first poll keeps unknown-head; Unix-only live
watcher fails closed at TUI startup; pane does not run without it);
`discover_and_collect` stores Unknown on `previous is None` without going
through collect; `(_, GitHead::Unknown) => continue` and `Unknown →
Commit` with `old = None`; Unborn is the healthy-empty marker. No third
`return ChangeResult` in collect — only the two sites R7-1 named. The
heads loop skips `git_failures` and on a cancelled git read keeps that
repo's previous head; it does not clone the whole previous snapshot.
`CancelAfterFirstLoad` increments on every `load` and asserts loads == 2,
so emptying `current.files` or dropping either cancel check fails that
pin — the probe does hit the second load when the production shape is
intact. A comment `if cancelled.load(Ordering::Acquire)` would change the
source pin's split count away from 2; it does not let `previous.clone()`
through. `tui_cmd::live` still starts the watcher before draw; `--demo`
never reaches `Pane::open`.

## Notes

- The source pin
  `failed_git_discovery_does_not_baseline_away_an_unknown_head` is
  M12b-style string-contains on a sliced `include_str!`. It **does**
  catch both cancel-site `previous.clone()` restores and requires exactly
  two `if cancelled.load(Ordering::Acquire)` returns. It does **not**
  catch (m5) a hollow helper, (F1) a stubbed `CollectCancel` impl for
  `AtomicBool`. The helper is covered by behavioural pins; the production
  impl is not.
- `cancelled_collect_keeps_unknown_for_a_late_failed_repository`
  constructs `AtomicBool::new(true)` with an empty `files` map. If
  `CollectCancel::load` never returns true, the `git_failures` loop has
  already inserted Unknown onto `next` and the function returns that
  `next` — the assert cannot tell cancel-retain from fall-through.
- `git status --porcelain` at start: `M README.md`, `M dev-diary/PLAN.md`,
  `M src/cli/watcher.rs`, `?? …/m12d-remediation-round6.md`,
  `?? …/m12d-remediation-round7.md`, `?? …/m12d-round7.md`. End: that set
  plus this file. `src/tui/` untouched. M12e/M12f untouched.
