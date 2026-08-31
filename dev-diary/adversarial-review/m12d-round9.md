# M12d round 9 — adversarial re-verification of the round-8 remediations

Reviewed against HEAD `14135e0`, branch `m12d-watcher`, tree dirty with the
round-6 through round-8 remediations (no commits). Scope: the two findings in
`m12d-remediation-round8.md` (M12d-R8-1 production `AtomicBool`
`CollectCancel` load/git-flag; M12d-R8-2 README fail-closed reject), the
named pins those remediations added, the R7 pins those remediations left in
place, and same-class residue (a discarded-load stub that still walks when
files are present, a hollow `AtomicBool::load` that returns the value but
ignores `Ordering`, `git_cancel_flag` aliases of `self`, a source pin gamed
by comments, `without our watcher` / comma-plus-determiner README restatements,
a live path that still starts the TUI without the watcher). All runs in a
clean env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u
DATABASE_URL`). Every mutation below was transient and reverted: the
touched file was restored from a byte copy taken immediately before the
mutation and `sha256sum`-verified identical afterwards (`src/cli/watcher.rs`
`6a43f9d0990b952e9145626234c3982c0d297df8d993df20fce56c7611f6418b`,
`README.md`
`7dc6003db7a268b41b4174986f2826f8544f9248a7b38ee3c50bfc209d17763b`,
`src/cli/tui_cmd.rs`
`bfa0dafc1c99e08092661bdfe24c1376e6039d9f78ab268d4f0cbfc92a65f98a`).
`git status --porcelain` at the end shows the same dirty set as at the
start (`README.md`, `dev-diary/PLAN.md`, `src/cli/watcher.rs`, untracked
`m12d-remediation-round6.md`, `m12d-remediation-round7.md`,
`m12d-remediation-round8.md`, `m12d-round7.md`, `m12d-round8.md`) plus this
record. Nothing committed by this round. `src/tui/` was not opened.
`move_agent_to_root` is unavailable to this subagent; work stayed in
`/tmp/mooshik-m12d-impl`.

## Verdict

**REMEDIATE** — R8-1's discarded-load and static-`git_cancel_flag` stubs
are genuinely closed and the named pins bite the mutations the remediation
named; R7's two cancel-site `previous.clone()` restores still fail their
pins. R8-2's claimed README mutations (`available without a watcher` and
`available, without the watcher`) fail as claimed, and `pane.close()` still
bites. Residue of the same class remains: the README reject still keys off
exact `without the watcher` / `without a watcher` plus an `available without`
… `watcher` split that a comma breaks, so `available, without our watcher`
(and `this` / `any`) stays green (F1). 1×P3.

## What held

- **R8-1 discarded load with files.** `CollectCancel::load` for `AtomicBool`
  evaluates `AtomicBool::load` and returns `false`. Fails
  `cancelled_atomicbool_with_files_keeps_unknown_without_walking` at
  `changed` with "a no-op AtomicBool load would walk the new file and look
  retained" and `atomicbool_collect_cancel_forwards_load_and_git_flag` at
  `CollectCancel::load for AtomicBool must forward to AtomicBool::load`.
  `cancelled_collect_keeps_unknown_for_a_late_failed_repository` stays
  green (empty `files`, Unknown already on `next`). 34 passed, 2 failed.
- **R8-1 static git flag.** `git_cancel_flag` returns a static
  `AtomicBool::new(false)`. Fails only the source pin at `git_cancel_flag
  must return self so git subprocesses see the production flag`. The new
  behavioural pin stays green (`load` still cancels before the walk).
  35 passed, 1 failed.
- **R8-1 stronger cousins that still bite.** `load` that returns
  `AtomicBool::load(self, Ordering::Relaxed)` and drops `order` fails the
  source pin (substring `AtomicBool::load(self, order) }`). `git_cancel_flag`
  returning `&*self` or `return self;` fails the same source pin (`->
  &AtomicBool { self }`). A comment spoof
  `/* AtomicBool::load(self, order) } */` plus `1 == 0` (no-op without the
  word `false`) leaves the source pin green and **fails the behavioural
  pin** — the file-walk probe is not vacuous. A `let v = AtomicBool::load
  (self, order); v` wrapper fails the source pin (not the claimed hole).
- **R7-1 first cancel still pinned.** Restoring only the first
  `cancelled.load` return to `previous.clone()` fails
  `cancelled_collect_keeps_unknown_for_a_late_failed_repository`,
  `cancelled_late_failed_repository_replays_first_commit_after_recovery`,
  the new AtomicBool-with-files pin (`None` vs `Some(Unknown)`), and the
  source pin at `cancel return 0`. File-walk pin stays green.
- **R7-1 file-walk cancel still pinned.** Restoring only the file-loop
  cancel return to `previous.clone()` fails
  `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository` and the
  source pin at `cancel return 1`.
- **R8-2 claimed README mutations.** Adding `The pane remains available
  without a watcher` or `The pane remains available, without the watcher`
  while keeping `fails closed at TUI startup` and `The watcher stops with
  the pane` fails `live_watching_fails_closed_at_tui_startup` at `README
  must not claim the pane runs without the watcher`. `available without
  our watcher` (no comma) also fails via the `available without` …
  `watcher` split. `without the file watcher` fails the same split.
- **R8-2 `pane.close()`.** Deleting `pane.close()` from the
  `Watcher::start` `Err` arm fails the same pin at `Watcher::start
  failure must close the pane`. `live` still
  `return Err(anyhow::Error::new(error))` after close. `--demo` still
  never reaches `Pane::open`; `live` still starts the watcher before
  draw. There is no live TUI path that opens the pane without the
  watcher.

## Findings

| # | Pri | File | Finding |
|---|---|---|---|
| F1 | P3 | `src/cli/watcher.rs` `readme_claims_available_without_watcher` | **The README reject is still three exact flattened phrases; a comma plus a determiner other than `the`/`a` restates the historical lie and stays green.** Executed: README keeps the true fail-closed sentences and adds `The pane remains available, without our watcher` — pin **passes** (36). Same with `available, without this watcher` and `Live watching is available, without any watcher`. Round 8 closed `without a watcher` and `available, without the watcher` (`without the watcher` still matches after the comma). The next dodge puts a comma between `available` and `without` (breaking `split_once("available without")`) and swaps the article for `our`/`this`/`any`. **Fix:** strip punctuation before matching, or treat `available` … `without` … `watcher` as an availability claim regardless of the determiner in between. |

## Mutation table

All runs in the clean env. Every mutation reverted and `sha256sum`-verified
byte-identical after its run.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | `impl CollectCancel for AtomicBool`: `load` evaluates `AtomicBool::load` and returns `false` | `cancelled_atomicbool_with_files_keeps_unknown_without_walking`; `atomicbool_collect_cancel_forwards_load_and_git_flag` | **both fail — bites** (`changed` walk; source `must forward`). Empty-files collect pin stays green |
| 2 | `git_cancel_flag` returns a static `AtomicBool::new(false)` | `atomicbool_collect_cancel_forwards_load_and_git_flag` | **fails — bites** (`must return self`). Behavioural pin stays green |
| 3 | README adds `The pane remains available without a watcher` (true sentences kept) | `live_watching_fails_closed_at_tui_startup` | **fails — bites** |
| 4 | README adds `The pane remains available, without the watcher` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** |
| 5 | README `The pane remains available without our watcher` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** (`available without` … `watcher`) |
| 6 | README `The pane remains available, without our watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — gap (F1)** |
| 7 | `load` returns `AtomicBool::load(self, Ordering::Relaxed)`, drops `order` | `atomicbool_collect_cancel_forwards_load_and_git_flag` | **fails — bites** (source). Behavioural pin stays green: the flag value is still returned |
| 8 | `git_cancel_flag` returns `&*self` | `atomicbool_collect_cancel_forwards_load_and_git_flag` | **fails — bites** (source; semantically the production flag) |
| 9 | comment spoof `AtomicBool::load(self, order) }` plus `1 == 0` | `cancelled_atomicbool_with_files_keeps_unknown_without_walking`; source pin | **behavioural fails — bites the no-op; source pin passes** (documented `include_str` limit, not a numbered finding) |
| 10 | first cancel return → `previous.clone()` | cancelled collect pins; AtomicBool-with-files pin; source pin | **fail — R7 still bites** (`cancel return 0`) |
| 11 | file-loop cancel return → `previous.clone()` | `cancelled_file_walk_keeps_unknown_for_a_late_failed_repository`; source pin | **fail — R7 still bites** (`cancel return 1`) |
| 12 | README `available, without this watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — same gap (F1)** |
| 13 | README `available, without any watcher` | `live_watching_fails_closed_at_tui_startup` | **passes — same gap (F1)** |
| 14 | README `available without the file watcher` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** the `available without` split |
| 15 | `live` `Err` arm drops `pane.close()` | `live_watching_fails_closed_at_tui_startup` | **fails — bites** |
| 16 | `load` is `let v = AtomicBool::load(self, order); v` | source pin | **fails — bites** (wrapper is not the claimed no-op) |
| 17 | `git_cancel_flag` is `return self;` | source pin | **fails — bites** |

## Gates (run by me, clean env)

- **`cargo test --locked --lib cli::watcher`** — **36 passed, 0 failed, 0
  ignored**, exit 0, on the restored tree after the last revert (forced
  rebuild after `include_str!` mutations so README/`watcher.rs` were not
  stale).
- **`cargo fmt --check`** — exit 0.
- **`cargo clippy --locked --all-targets --all-features -- -D warnings`**
  — exit 0, zero warnings.
- **File-size:** `watcher.rs` 2625 lines — dedicated M12d module over the
  1000-line cap used elsewhere; not split this round.

## Executed vs read

**Executed:** the seventeen mutations above (each reverted and hash-verified);
the named R8 and R7 pins on the clean tree and after each mutation; the
watcher lib filter (36) on the restored tree. Non-Unix `Watcher::start` is
source-pinned, not run on this Linux host.

**Read (not re-executed):** PLAN.md §M12d; `tui_cmd::live` still starts
`Watcher::start` after `Pane::open` and closes the pane on `Err`; `--demo`
draws a fixed scene and never reaches `Pane::open`. `readme_claims_available_without_watcher`
lowercases whitespace-joined text, then `contains("without the watcher")` /
`contains("without a watcher")`, else `split_once("available without")` and
`rest.contains("watcher")`. A comma glued to `available,` prevents that
split. No third `return ChangeResult` in `collect_changes_with_cancel`.

## Notes

- The source pin
  `atomicbool_collect_cancel_forwards_load_and_git_flag` is M12b-style
  string-contains on a sliced `include_str!`. It **does** catch discarded
  `false`, `AtomicBool::new` in `git_cancel_flag`, `&*self`, and
  `return self;`. It does **not** catch (m9) a comment that repeats the
  required call while the body no-ops without the token `false`. The
  behavioural pin with a file in `current.files` catches that no-op. An
  always-true comment spoof would still cancel and stay green on both
  pins; that is fail-closed, not the F1 hole from round 8.
- `Ordering` is source-pinned to the `order` parameter. A Relaxed load of
  the same flag still cancels the walk in the behavioural fixture; not a
  finding.
- `git status --porcelain` at start: `M README.md`, `M dev-diary/PLAN.md`,
  `M src/cli/watcher.rs`, `?? …/m12d-remediation-round6.md`,
  `?? …/m12d-remediation-round7.md`, `?? …/m12d-remediation-round8.md`,
  `?? …/m12d-round7.md`, `?? …/m12d-round8.md`. End: that set plus this
  file. `src/tui/` untouched. M12e/M12f untouched.
