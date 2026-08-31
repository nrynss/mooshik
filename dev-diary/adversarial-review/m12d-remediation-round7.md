# M12d round-7 remediation

Remediates the two findings from `m12d-round7.md` — P2 (the file-walk
cancel return was unpinned and `previous.clone()` there reopened the
R6-1 Unknown drop) and P3 (the README fail-closed conjunct was an
exact-phrase `contains`) — with no deferrals. Base: branch
`m12d-watcher` at `14135e0`; round-6 remediations already in the dirty
tree were kept, not reverted. The tree is left dirty for the
orchestrator, nothing committed. All runs in a clean env (`env -u
LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`). Every
mutation below was transient and reverted: the touched file was restored
from a byte copy taken immediately before the mutation and
`sha256sum`-verified identical afterwards (`src/cli/watcher.rs`
`40ed69cd2fb07467de2a3dd418e8d06822ca240d2a19620aa18793c116dedc5b`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`,
`src/cli/tui_cmd.rs`
`bfa0dafc1c99e08092661bdfe24c1376e6039d9f78ab268d4f0cbfc92a65f98a`).

## M12d-R7-1 (P2) — both cancel returns retain Unknown, including the file walk

### What was wrong

Round 6 made the *first* `cancelled.load` return
`snapshot_retaining_failed_discoveries(previous, current)` and pinned
that slice. Production already used the helper in the `for (path, state)`
return as well, but that second site was invisible: the source pin
stopped at `for (path, state)`, and both cancelled behavioural pins
constructed `AtomicBool::new(true)` up front, so they never entered the
file loop. Restoring only the file-walk return to `previous.clone()`
left every named R6-1 pin green. A pane stop after the first check and
during the walk dropped Unknown for a late failed repo; the next healthy
poll baselined history that existed during the failure — the R6-1 hole,
one statement later.

There is no third cancel/`previous.clone()` return in
`collect_changes_with_cancel`. The heads loop skips `git_failures` and
on a cancelled git read keeps that repo's previous head; it does not
clone the whole previous snapshot.

### The fix

Both cancel returns still use
`snapshot_retaining_failed_discoveries(previous, current)`. Collect is
generic over a small `CollectCancel` probe so a test flag can be false
at the first load and true on the next (file-walk) load; production
still passes `&AtomicBool`. Git subprocess cancel continues to use the
underlying `AtomicBool`.

The source pin now takes **each** `if cancelled.load(Ordering::Acquire)`
block through its `return ChangeResult` and requires the helper call
(and forbids `previous.clone()`) in both. It also requires there are
exactly two such returns.

### The pins that bite

Each mutation transient, reverted, `sha256sum`-verified against the
pre-mutation copy.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | file-loop cancel return → `previous.clone()` (first cancel left on the helper) | `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository`, `failed_git_discovery_does_not_baseline_away_an_unknown_head` | **caught** — `32 passed; 2 failed`. Behavioural pin: `left: None right: Some(Unknown)` with "file-walk cancel must not drop a failed discovery that already ran" (loads == 2, so the first check was false). Source pin: `cancel return 1 must retain Unknown markers from git_failures, not return previous.clone()`. The six named R6-1 pins that ignored this site still pass, as round 7 showed. |

## M12d-R7-2 (P3) — README fail-closed is not an exact-phrase `contains`

### What was wrong

`live_watching_fails_closed_at_tui_startup` rejected only the historical
sentence `TUI remains available without the watcher`. Restating as `The
pane remains available without the watcher` stayed green while
production `start` / `live` conjuncts still held.

### The fix

The same pin now requires flattened README text to contain `fails closed
at TUI startup` and the literal `The watcher stops with the pane`, and
rejects any `available without the watcher` substring
(case-insensitive). `pane.close()` and `return Err(anyhow::Error::new(error))`
in the `Watcher::start` `Err` arm are unchanged. No `src/tui/` files
touched; README production text was already the round-6 wording.

### The pins that bite

| # | Mutation | Pin | Result |
|---|---|---|---|
| 2 | README keeps the true fail-closed sentences and adds `The pane remains available without the watcher` | `live_watching_fails_closed_at_tui_startup` | **caught** — `0 passed; 1 failed`; `README must not claim the pane runs without the watcher`. |
| 3 | `live` `Err` arm drops `pane.close()` | `live_watching_fails_closed_at_tui_startup` | **caught** — `0 passed; 1 failed`; `Watcher::start failure must close the pane`. Forced rebuild after revert so `include_str!` for `tui_cmd.rs` was not a stale artifact. |

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN` /
`MOOSHIK_POSTGRES_DSN` / `DATABASE_URL` unset):

* `cargo test --locked --lib cli::watcher` → **34 passed, 0 failed, 0
  ignored**, exit 0 (forced rebuild after the last revert).
* `cargo fmt --check` → clean.
* `cargo clippy --locked --all-targets --all-features -- -D warnings`
  → clean, exit 0.
* File-size: `watcher.rs` 2531 lines — dedicated M12d module over the
  1000-line cap used elsewhere; not split this round.

## What was executed vs. only read

**Executed.** Mutation 1 (file-loop cancel → `previous.clone()`),
mutation 2 (README `available without the watcher` restatement),
mutation 3 (`pane.close()` dropped from the live `Err` arm), each
reverted and hash-verified. Green watcher suite before the mutations
and after the last revert. fmt, clippy.

**Read, not executed.** Non-Unix `Watcher::start` on this Linux host
(the `#[cfg(not(unix))]` arm is source-pinned, not run). A third cancel
return in collect: there isn't one.

**Residue not fixed.** None of the same class. `src/tui/` was not
opened. M12e/M12f untouched.
