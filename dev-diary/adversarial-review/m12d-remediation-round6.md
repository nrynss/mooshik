# M12d round-6 remediation

Remediates the two findings from the reconstructed round-6 review — P2
(a late-appearing repo whose first Git read fails is silently dropped
instead of `Unknown`) and P3 (README overstates fail-closed) — plus the
same-class residue that review named: cancel during a failed discover,
`git_failures` without a paired head entry, and PLAN/start comments that
still talked as if only the *initial* discovery kept `Unknown` or as if
the pane ran without the watcher. No deferrals. Base: branch
`m12d-watcher` at `14135e0`; the tree is left dirty for the orchestrator,
nothing committed. All runs in a clean env (`env -u LAMBO_POSTGRES_DSN
-u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`). Every mutation below was
transient and reverted: the touched file was restored from a byte copy
taken immediately before the mutation and `sha256sum`-verified identical
afterwards (`src/cli/watcher.rs`
`e11051357243e038e98dd3c352fb9ad12c477137a8fd20213aedea603a86ebbe`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`).

WIP already in the worktree (Unknown insert + nested recovery test +
README "fails closed at TUI startup") was kept and completed, not
reverted.

## M12d-R6-1 (P2) — a late failed repository keeps Unknown, including across cancel

### What was wrong

`collect_changes_with_cancel` kept the previous head for a known repo
that failed, but for `current.git_failures` without a `previous.heads`
entry HEAD still did `next.heads.remove(repo)`. Discover had already
inserted `GitHead::Unknown`; the remove dropped it. The next healthy
poll then saw a repo with no previous head and baselined it as new —
history that existed during the failure never replayed. PLAN says
failed discovery keeps Unknown and replays on recovery; only a
genuinely new *healthy* repository is baselined. 14135e0 closed the
startup-initial case (`failed_initial_git_discovery_replays_history_after_recovery`);
the late-appearing poll was the same hole one step later.

WIP already inserted `GitHead::Unknown` and added
`late_failed_repository_keeps_unknown_marker_for_first_commit_recovery`.
That was the right production change and was not enough: a cancelled
collect still returned `previous.clone()`, which dropped the same
marker, so the next healthy poll baselined again. The `git_failures`
list without a paired `heads` entry also needed the insert (clone
cannot carry a head that was never recorded).

### The fix

The git-failures else-arm keeps `GitHead::Unknown` instead of removing
the head. Cancel returns
`snapshot_retaining_failed_discoveries(previous, current)`: previous
file/head state (no partial walk), plus Unknown for any failed repo
that previous did not already know. Nested recovery still walks
`Unknown → Commit` with `old = None` and enqueues the first commit(s).
Unborn stays the healthy-empty marker; it is not used for a failed
read. Startup (`previous is None`) was already Unknown from 14135e0.

`dev-diary/PLAN.md` now says a failed Git discovery *including a
repository that appears after the first poll* retains Unknown; only a
genuinely new healthy repository is baselined.

### The pins that bite

Each mutation transient, reverted, `sha256sum`-verified against the
pre-mutation copy.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | restore `next.heads.remove(repo)` in the git-failures else-arm | `late_git_failure_without_a_previous_head_keeps_unknown`, `late_failed_repository_keeps_unknown_marker_for_first_commit_recovery`, `git_failure_list_without_a_head_entry_still_records_unknown`, `failed_git_discovery_does_not_baseline_away_an_unknown_head` | **caught** — `0 passed; 4 failed`. Behavioural pins: `left: None right: Some(Unknown)` with "removing the head would baseline history that existed during the failure". Source pin: `a late failed repository must keep Unknown` (the else-arm no longer names `GitHead::Unknown`; `!failures.contains("heads.remove")` is the same arm). Recovery never runs: the marker is already gone. |
| 2 | cancel returns `previous.clone()` instead of `snapshot_retaining_failed_discoveries` | `cancelled_collect_keeps_unknown_for_a_late_failed_repository`, `cancelled_late_failed_repository_replays_first_commit_after_recovery`, source pin | **caught** — `0 passed; 3 failed`. `cancel must not drop a failed discovery that already ran`; source pin: `cancel must retain Unknown markers from git_failures, not return previous.clone()`. |

On the shipped form, recovery after a late failed nested repo (and after
a cancelled failed collect) enqueues `first commit after recovery` /
`first commit after cancelled failure`. Unborn nested repos still do
not block files or their first commit
(`unborn_nested_repository_does_not_block_files_or_its_first_commit`).
Corrupt HEAD is still Unknown, not Unborn.

## M12d-R6-2 (P3) — live watching fails closed at TUI startup

### What was wrong

HEAD README said live watching fails closed and "the TUI remains
available without the watcher". `tui_cmd::live` closes the pane and
returns the watcher error on `Watcher::start` failure —
`WatchError::WorkspaceUnavailable` for non-Unix and canonicalize /
not-a-directory. The TUI does not remain available.

WIP already said "fails closed at TUI startup". That matches
`Watcher::start` and `live`. Residue: PLAN still only named "failed
*initial* Git discovery" and did not say the pane does not run without
the watcher; the non-Unix comment in `start` did not say `live` closes
the pane.

### The fix

README: "fails closed at TUI startup. The watcher stops with the pane."
`Watcher::start`'s non-Unix arm now says the live command closes the
pane and returns the error. PLAN: Unix-only live watcher fails closed
at TUI startup; the pane does not run without it. `src/text/en.toml`
already tells the operator to run `mooshik tui` again — left alone.
No `src/tui/` files touched; `tui_cmd.rs` already closes the pane
(comment + `pane.close()` + `return Err`).

### The pins that bite

| # | Mutation | Pin | Result |
|---|---|---|---|
| 3 | restore README "the TUI remains available without the watcher" | `live_watching_fails_closed_at_tui_startup` | **caught** — `0 passed; 1 failed`; `README must not claim the pane runs without the watcher`. The same pin also requires `#[cfg(not(unix))]` + `WatchError::WorkspaceUnavailable` in `start`, and `pane.close()` + `return Err(anyhow::Error::new(error))` in `live` after `Watcher::start`. |

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN` /
`MOOSHIK_POSTGRES_DSN` / `DATABASE_URL` unset):

* `cargo test --locked --lib cli::watcher` → **33 passed, 0 failed, 0
  ignored**, exit 0.
* `cargo fmt --check` → clean.
* `cargo clippy --locked --all-targets --all-features -- -D warnings`
  → clean, exit 0.
* `cargo test --locked` → exit 0. Lib `--list` reports **615 tests**
  plus **2 ignored**; `tests/report_pin.rs` 1 passed; 0 doc tests.
  Watcher filter **33 passed, 0 failed, 0 ignored** (six new pins plus
  the WIP recovery test on top of 14135e0's watcher suite).
* File-size: `watcher.rs` 2421 lines — already the dedicated M12d
  module over the 1000-line cap used elsewhere; not split.

## What was executed vs. only read

**Executed.** Mutation 1 (`next.heads.remove(repo)`), mutation 2
(cancel → `previous.clone()`), mutation 3 (README restore), each
reverted and hash-verified. Green watcher suite before the mutations
and after the last revert. fmt, clippy, full locked test.

**Read, not executed.** Non-Unix `Watcher::start` on this Linux host
(the `#[cfg(not(unix))]` arm is source-pinned, not run). A workspace
root that *becomes* a git repo mid-session uses the same
`collect_changes` path as nested; pinned with constructed
`PathBuf::from("/workspace/late")` snapshots rather than a second
filesystem fixture.

**Residue not fixed.** None of the same class. `src/tui/` was not
opened. M12e/M12f untouched.
