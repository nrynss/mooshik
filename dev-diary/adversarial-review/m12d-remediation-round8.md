# M12d round-8 remediation

Remediates the two findings from `m12d-round8.md` — P3 (production
`AtomicBool` `CollectCancel` can no-op `load` or substitute a never-set
git flag while every named R7-1 pin stays green) and P3 (the README
reject is still an exact five-word phrase) — with no deferrals. Base:
branch `m12d-watcher` at `14135e0`; round-6 and round-7 remediations
already in the dirty tree were kept, not reverted. The tree is left
dirty for the orchestrator, nothing committed. All runs in a clean env
(`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`).
Every mutation below was transient and reverted: the touched file was
restored from a byte copy taken immediately before the mutation and
`sha256sum`-verified identical afterwards (`src/cli/watcher.rs`
`6a43f9d0990b952e9145626234c3982c0d297df8d993df20fce56c7611f6418b`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`,
`src/cli/tui_cmd.rs`
`bfa0dafc1c99e08092661bdfe24c1376e6039d9f78ab268d4f0cbfc92a65f98a`).

## M12d-R8-1 (P3) — production AtomicBool CollectCancel is pinned, not only the probe

### What was wrong

Round 7 made collect generic over `CollectCancel` so a test flag can be
false at the first load and true on the file-walk load. Production still
passes `&AtomicBool`, and the impl already forwarded `load` to
`AtomicBool::load` and returned `self` from `git_cancel_flag`. None of
the named pins required that. `cancelled_file_walk_*` uses
`CancelAfterFirstLoad`. `cancelled_collect_*` passes
`AtomicBool::new(true)` with an empty `files` map, so Unknown is already
on `next` from the `git_failures` loop; a `load` that always returns
false still looks retained. Executed: `load` evaluates the real load and
discards it (`false`); `git_cancel_flag` returns a static
`AtomicBool::new(false)`. All 34 watcher tests stayed green. A pane stop
during collect would no longer abort the file walk or git subprocess via
the production impl.

### The fix

Production `impl CollectCancel for AtomicBool` is unchanged. Two pins
now require it.

`cancelled_atomicbool_with_files_keeps_unknown_without_walking` passes
`&AtomicBool::new(true)` with a new file in `current.files`. Cancel must
keep previous file state, not enqueue the walk, and still retain
Unknown. A no-op `load` walks the file (`changed`, pending, files on
`next`) and fails even though git_failures already inserted Unknown.

`atomicbool_collect_cancel_forwards_load_and_git_flag` slices
`impl CollectCancel for AtomicBool` and requires `load` to forward to
`AtomicBool::load(self, order)` as the return (not a discarded call plus
`false`) and `git_cancel_flag` to return `self` (not
`AtomicBool::new`).

The empty-files first-cancel pin and the `CancelAfterFirstLoad`
file-walk pin are unchanged.

### The pins that bite

Each mutation transient, reverted, `sha256sum`-verified against the
pre-mutation copy.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | `impl CollectCancel for AtomicBool`: `load` evaluates `AtomicBool::load` and returns `false` | `cancelled_atomicbool_with_files_keeps_unknown_without_walking`, `atomicbool_collect_cancel_forwards_load_and_git_flag` | **caught** — `34 passed; 2 failed`. Behavioural pin: `changed` with "a no-op AtomicBool load would walk the new file and look retained". Source pin: `CollectCancel::load for AtomicBool must forward to AtomicBool::load`. `cancelled_collect_keeps_unknown_for_a_late_failed_repository` stays green (empty `files`, Unknown already on `next`). |
| 2 | `git_cancel_flag` returns a static `AtomicBool::new(false)` | `atomicbool_collect_cancel_forwards_load_and_git_flag` | **caught** — `35 passed; 1 failed`; `git_cancel_flag must return self so git subprocesses see the production flag`. The new behavioural pin stays green: `load` still cancels before git. |

## M12d-R8-2 (P3) — README reject is not one five-word phrase

### What was wrong

`live_watching_fails_closed_at_tui_startup` rejected only the flattened
substring `available without the watcher`. Restating as `The pane
remains available without a watcher` (`the` → `a`) or `The pane remains
available, without the watcher` (comma) stayed green while the true
fail-closed sentences and `pane.close()` still held.

### The fix

The same pin still requires `fails closed at TUI startup` and the
literal `The watcher stops with the pane`, and `pane.close()` plus
`return Err(anyhow::Error::new(error))` in the `Watcher::start` `Err`
arm. The README reject now treats flattened lowercase text as an
availability claim if it contains `without the watcher` or `without a
watcher`, or `available without` later followed by `watcher`. README
production text was already the round-6 wording. No `src/tui/` files
touched.

### The pins that bite

| # | Mutation | Pin | Result |
|---|---|---|---|
| 3 | README keeps the true fail-closed sentences and adds `The pane remains available without a watcher` | `live_watching_fails_closed_at_tui_startup` | **caught** — `35 passed; 1 failed`; `README must not claim the pane runs without the watcher`. |
| 4 | README keeps the true sentences and adds `The pane remains available, without the watcher` | `live_watching_fails_closed_at_tui_startup` | **caught** — `35 passed; 1 failed`; same assertion. |

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN` /
`MOOSHIK_POSTGRES_DSN` / `DATABASE_URL` unset):

* `cargo test --locked --lib cli::watcher` → **36 passed, 0 failed, 0
  ignored**, exit 0 (forced rebuild after the last revert so `include_str!`
  for README was not a stale artifact).
* `cargo fmt --check` → clean.
* `cargo clippy --locked --all-targets --all-features -- -D warnings`
  → clean, exit 0.
* File-size: `watcher.rs` 2625 lines — dedicated M12d module over the
  1000-line cap used elsewhere; not split this round.

## What was executed vs. only read

**Executed.** Mutation 1 (`load` discarded → `false`), mutation 2
(`git_cancel_flag` static never-set flag), mutation 3 (README `without a
watcher`), mutation 4 (README comma + `without the watcher`), each
reverted and hash-verified. Green watcher suite before the mutations
and after the last revert. fmt, clippy.

**Read, not executed.** Non-Unix `Watcher::start` on this Linux host
(the `#[cfg(not(unix))]` arm is source-pinned, not run).

**Residue not fixed.** None of the same class. `src/tui/` was not
opened. M12e/M12f untouched.
