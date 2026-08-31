# M12d round 7 — adversarial re-verification of the round-6 remediations

Reviewed against HEAD `14135e0`, branch `m12d-watcher`, tree dirty with the
round-6 remediations (no commits). Scope: the two findings in
`m12d-remediation-round6.md` (M12d-R6-1 late-failed Unknown, including
cancel; M12d-R6-2 fail-closed at TUI startup), the six named behavioural
pins plus the source pin and the README/start/`live` conjuncts, and
same-class residue the remediations claimed to have closed (a second
`previous.clone()` cancel path, `git_failures` without a paired head,
Unborn vs Unknown, nested vs constructed late repo, fail-closed docs vs
`tui_cmd::live`). All runs in a clean env (`env -u LAMBO_POSTGRES_DSN
-u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`). Every mutation below was
transient and reverted: the touched file was restored from a byte copy
taken immediately before the mutation and `sha256sum`-verified identical
afterwards (`src/cli/watcher.rs`
`e11051357243e038e98dd3c352fb9ad12c477137a8fd20213aedea603a86ebbe`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`,
`src/cli/tui_cmd.rs`
`bfa0dafc1c99e08092661bdfe24c1376e6039d9f78ab268d4f0cbfc92a65f98a`).
`git status --porcelain` at the end shows the same dirty set as at the
start (`README.md`, `dev-diary/PLAN.md`, `src/cli/watcher.rs`, untracked
`m12d-remediation-round6.md`) plus this record. Nothing committed by
this round. `src/tui/` was not opened.

## Verdict

**REMEDIATE** — R6-1's else-arm and the *first* cancel return are
genuinely fixed and the named pins bite those two sites, and R6-2's
production fail-closed (`Watcher::start` non-Unix refusal, `live`
`pane.close()` + `return Err`) is real and pinned. Residue of the same
class remains: `collect_changes_with_cancel` has a second cancel return
inside the file walk that still accepts `previous.clone()`, and every
named R6-1 pin stays **green** under that mutation (F1). The README
conjunct of the fail-closed pin is an exact-phrase `contains` and a
restated false claim (`The pane remains available without the watcher`)
stays green (F2). 1×P2 + 1×P3.

## What held

- **R6-1 else-arm — a late failed repository keeps Unknown.** Restoring
  `next.heads.remove(repo)` fails all four named pins:
  `late_git_failure_without_a_previous_head_keeps_unknown` and
  `late_failed_repository_keeps_unknown_marker_for_first_commit_recovery`
  at `left: None right: Some(Unknown)` with "removing the head would
  baseline history that existed during the failure";
  `git_failure_list_without_a_head_entry_still_records_unknown` the same
  `None` vs `Some(Unknown)`; source pin
  `failed_git_discovery_does_not_baseline_away_an_unknown_head` at
  `a late failed repository must keep Unknown`. Recovery after a nested
  late failure still enqueues `first commit after recovery`. Unborn
  instead of Unknown in the else-arm fails the same behavioural pins
  (`Some(Unborn)` vs `Some(Unknown)`) and the source pin (the slice no
  longer names `GitHead::Unknown`). Corrupt HEAD is still Unknown, not
  Unborn. Startup (`previous is None`) still replays after recovery
  (`failed_initial_git_discovery_replays_history_after_recovery`).
  Unborn nested repos still do not block files or their first commit.
- **R6-1 first cancel return.** Replacing the *first*
  `cancelled.load` return with `previous.clone()` fails
  `cancelled_collect_keeps_unknown_for_a_late_failed_repository` and
  `cancelled_late_failed_repository_replays_first_commit_after_recovery`
  (`cancel must not drop a failed discovery that already ran`) and the
  source pin (`cancel must retain Unknown markers from git_failures, not
  return previous.clone()`). Emptying `snapshot_retaining_failed_discoveries`
  to a bare `previous.clone()` (call site left in place) fails both
  cancelled behavioural pins — those pins are not vacuous for the helper
  body. `tui_cmd::live` still starts the watcher before draw; the demo
  `--scene` path is not live watching.
- **R6-2 production fail-closed.** README restored to "the TUI remains
  available without the watcher" fails `live_watching_fails_closed_at_tui_startup`
  at `README must not claim the pane runs without the watcher`. Deleting
  `pane.close()` from the `Watcher::start` `Err` arm fails the same pin
  at `Watcher::start failure must close the pane`. `live` still
  `return Err(anyhow::Error::new(error))` after close; `start`'s
  `#[cfg(not(unix))]` arm still returns `WatchError::WorkspaceUnavailable`.
  PLAN and the `start` comment already say the pane does not run without
  the watcher.

## Findings

| # | Pri | File | Finding |
|---|---|---|---|
| F1 | P2 | `src/cli/watcher.rs` file-loop cancel (~497–503); source pin ~1942–1952 | **The mid-walk cancel return is unpinned — `previous.clone()` there reintroduces the R6-1 baseline hole with every named pin green.** Executed: only the `for (path, state)` cancel return restored to `previous.clone()` (the first `cancelled.load` return and the else-arm Unknown insert left intact). Result: `late_git_failure_without_a_previous_head_keeps_unknown`, `late_failed_repository_keeps_unknown_marker_for_first_commit_recovery`, `git_failure_list_without_a_head_entry_still_records_unknown`, `failed_git_discovery_does_not_baseline_away_an_unknown_head`, `cancelled_collect_keeps_unknown_for_a_late_failed_repository`, `cancelled_late_failed_repository_replays_first_commit_after_recovery` all **pass**. Discover can already have recorded a late failed repo; collect then walks files and checks the flag per path. A pane stop that lands after the first `cancelled.load` and during that walk returns `previous` without the Unknown marker, and the next healthy poll baselined the history that existed during the failure — the exact R6-1 bug, one statement later. The source pin splits the first `if cancelled.load(Ordering::Acquire)` only, through `for (path, state)`, so the second site is invisible to it; both behavioural cancel tests construct `AtomicBool::new(true)` up front and never enter the file loop. **Fix:** both cancel returns must keep `snapshot_retaining_failed_discoveries(previous, current)`; extend the source pin so *each* cancel block in `collect_changes_with_cancel` contains that call (or a behavioural pin whose flag flips after the first load). |
| F2 | P3 | `src/cli/watcher.rs` `live_watching_fails_closed_at_tui_startup` README conjunct | **The README half of the fail-closed pin is an exact-phrase `contains` and does not catch a restated false claim.** Executed: README set to `The pane remains available without the watcher` (the historical lie, `TUI` → `pane`). The pin stays **green**; production `start`/`live` conjuncts still hold. The claimed mutation (the exact HEAD sentence) does bite; a one-word respell of the same claim does not. **Fix:** pin a positive assertion (`fails closed at TUI startup` / `The watcher stops with the pane`) or reject any `available without the watcher` substring, not only `TUI remains available without the watcher`. |

## Mutation table

All runs in the clean env. Every mutation reverted and `sha256sum`-verified
byte-identical after its run.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | restore `next.heads.remove(repo)` in the git-failures else-arm | `late_git_failure_without_a_previous_head_keeps_unknown`; `late_failed_repository_keeps_unknown_marker_for_first_commit_recovery`; `git_failure_list_without_a_head_entry_still_records_unknown`; `failed_git_discovery_does_not_baseline_away_an_unknown_head` | **all four fail — bites** (`None` vs `Some(Unknown)`; source pin `must keep Unknown`) |
| 2 | first cancel return → `previous.clone()` | `cancelled_collect_keeps_unknown_for_a_late_failed_repository`; `cancelled_late_failed_repository_replays_first_commit_after_recovery`; source pin | **all three fail — bites** (`cancel must not drop a failed discovery`; source pin `not return previous.clone()`) |
| 3 | README restore "the TUI remains available without the watcher" | `live_watching_fails_closed_at_tui_startup` | **fails — bites** (`README must not claim the pane runs without the watcher`) |
| 4 | file-loop cancel return → `previous.clone()` (first cancel left correct) | all six R6-1 named pins | **all pass — gap (F1)** |
| 5 | else-arm insert `GitHead::Unborn` | the four else-arm pins | **all fail — bites** Unborn/Unknown mixup |
| 6 | else-arm insert Unborn with a `// GitHead::Unknown` comment | source pin; `late_git_failure_without_a_previous_head_keeps_unknown` | **source pin passes (string-contains vacuity); behavioural pin fails.** Documented limit of the source pin, not a numbered finding: the production mixup is still caught |
| 7 | `snapshot_retaining_failed_discoveries` body → `previous.clone()` (call site kept) | cancelled behavioural pins; source pin | **behavioural pins fail — bites the helper; source pin passes** (it only requires the call name). Same documented limit as m6 |
| 8 | README `The pane remains available without the watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — gap (F2)** |
| 9 | `live` `Err` arm drops `pane.close()` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** (`Watcher::start failure must close the pane`) |

## Gates (run by me, clean env)

- **`cargo test --locked --lib cli::watcher`** — **33 passed, 0 failed, 0
  ignored**, exit 0, on the restored tree after the last revert (forced
  rebuild so `include_str!` for `tui_cmd.rs` was not a stale artifact from
  mutation 9).
- **`cargo fmt --check`** — exit 0.
- **`cargo clippy --locked --all-targets --all-features -- -D warnings`**
  — exit 0, zero warnings.
- **File-size:** `watcher.rs` 2421 lines — dedicated M12d module over the
  1000-line cap used elsewhere; not split this round.

## Executed vs read

**Executed:** the nine mutations above (each reverted and hash-verified);
the named pins on the clean tree and after each mutation; the watcher
lib filter (33) on the restored tree. Non-Unix `Watcher::start` is
source-pinned, not run on this Linux host.

**Read (not re-executed):** PLAN.md §M12d (failed discovery including a
repo that appears after the first poll keeps unknown-head; Unix-only
live watcher fails closed at TUI startup; pane does not run without it);
`discover_and_collect` stores Unknown on `previous is None` without
going through collect; `(_, GitHead::Unknown) => continue` and
`Unknown → Commit` with `old = None`; Unborn is the healthy-empty
marker. No third `previous.clone()` cancel return in collect — only the
two sites, of which the second is F1. Nested recovery and constructed
`PathBuf::from("/workspace/late")` share `collect_changes_with_cancel`;
a workspace root that *becomes* a git repo mid-session is the same
else-arm.

## Notes

- The source pin `failed_git_discovery_does_not_baseline_away_an_unknown_head`
  is M12b-style string-contains on a sliced `include_str!`. It **does**
  catch the claimed `heads.remove` restore and the first-cancel
  `previous.clone()` restore. It does **not** catch (m6) a comment that
  names `GitHead::Unknown` while inserting Unborn, (m7) a hollow helper
  whose call site still matches, or (F1) the second cancel site. The
  first two are covered by behavioural pins; F1 is not.
- `git status --porcelain` at start: `M README.md`, `M dev-diary/PLAN.md`,
  `M src/cli/watcher.rs`, `?? …/m12d-remediation-round6.md`. End: that
  set plus this file. `src/tui/` untouched. M12e/M12f untouched.
