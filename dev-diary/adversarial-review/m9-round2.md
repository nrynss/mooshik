# M9 round 2 — adversarial re-verification of the remediation

Reviewed at `1f3217e` (branch `m9-measurement`), worktree `/tmp/mooshik-m9`.
Independent trace **and** mutation of each round-1 closure; hunt for new
residue in the ten new pins. Every mutation was transient and reverted;
tree verified clean (`git status` empty) after each.

## Closure verification

### P1-1 — EOF hang (`grade.py:118-124`) — CLOSED

Traced the branch: `answer_raw = infile.readline()` is checked for `""`
**before** strip/lower/KEYMAP, so a closed stdin returns like `q`; a blank
typed line arrives as `"\n"`, strips to `""`, misses every branch and
reprompts — the two inputs are distinct strings at the decision point.

* **Mutation M1**: EOF branch neutralised (`if False and answer_raw == ""`).
  Both pins fail fast via the 64-read guard —
  `test_eof_quits_instead_of_reprompting_forever` (one prompt seen, guard
  fires) and `test_eof_via_cli_still_persists_grades`. No hang.
* **Probe (distinctness)**: feeding `\n`, `"  \n"` reprompts twice then
  accepts a verdict (done=1); blank lines followed by real `""` EOF quit
  with reads=2. Blank ≠ EOF confirmed by execution.

### P2-2 — Ctrl-C lost verdicts (`__main__.py:117-122`) — CLOSED

Traced all three paths: interactive loop wrapped in `try/finally` —
`save_grades` runs on normal return, on `KeyboardInterrupt` propagation,
and would run on any clean error inside the loop. Template path returns
before the loop (nothing to save); apply path saves explicitly at line
113.

* **Mutation M2**: save moved back after the call (no try/finally). The
  interrupt pin fails (`KeyError` — sidecar empty after two recorded
  verdicts).
* **Probe (end-to-end)**: `main(["grade", …])` with stdin raising
  `KeyboardInterrupt` after two verdicts → `SystemExit(130)` **and**
  both verdicts in the sidecar. Normal `q` → exit 0. Exit code 130
  preserved by the `except KeyboardInterrupt` in `main()`; the `finally`
  does not swallow the interrupt.

### P2-3 — JOIN fan-out duplication (`pools.py`) — CLOSED

Two independent layers traced:

1. `SELECT DISTINCT` in both pool queries collapses fully identical rows
   only — a two-parent concept yields rows differing in `source_ref`, so
   DISTINCT alone is provably insufficient here.
2. `_dedupe_by_node_id` keeps the first occurrence per `node_id`
   (`setdefault`); order is stable because SQL `ORDER BY c.id` feeds it
   and dict insertion order preserves it — deterministic, no hash-order
   dependence.

Pool-size semantics are consistent: `run_sampling` sizes and
`_cmd_report` sizes both go through the deduped `raw_pool` /
`rejected_pool`, so sizes count distinct nodes before *and* after the fix
consumers never see raw row counts.

* **Mutation M3**: `_dedupe_by_node_id` turned into a passthrough (DISTINCT
  still active). Fan-out pin fails exactly as claimed:
  `assert 2 == 1` on duplicate node count — drawn twice into one sample.
* **Probes**: dedup output byte-stable across 200 runs including a fan-out
  duplicate mid-list (first occurrence wins, order preserved);
  sampling/report sizes agree (= 3 nodes from 4 rows);
  `COVERAGE_SQL` structurally join-free (no JOIN, single FROM) and guarded
  by `test_coverage_sql_is_join_free`.

## New-residue hunt

| Check | Result |
| --- | --- |
| Unsure-exclusion pin checks interval **values**, not just n | Clean. Pin asserts the full row `… \| 2 \| 2 \| 100.0% \| [lo, hi] \|` against `wilson(2,2)` computed independently in the test. **Mutation M4** (unsure into denominator) fails it on values: rendered `[0.208, 0.939]` vs expected `[0.342, 1.000]` while n stays 2. |
| Negative-N pins hit argparse, not rng | Clean. Pins call `_build_parser().parse_args(...)` only; `_non_negative_int` raises during parsing. Probe monkeypatched `draw` to explode — negative N exits `SystemExit(2)` with usage message, `draw` never invoked; zero accepted, defaults intact. |
| Dict/set iteration-order flake risk | None found. Dedup uses dict insertion order (language-guaranteed since 3.7) over SQL-ordered input; sets used only for membership (`drawn_ids`); `save_grades` sorts; report iterates a fixed tuple. Full suite green under `PYTHONHASHSEED` ∈ {0, 1, 12345, 987654321}. Residual note (pre-existing, not new): cross-*version* determinism of `Random(str)` + `sample` is assumed, same mechanism the round-1-approved sampling pins already rely on. |
| Suite counts coherent | measurement 39 ✓ (29 + 10 new pins, counted in diff), ingester 36 ✓, cargo 194 passed + 1 integration, 1 ignored ✓ (gates below). |
| Rust changes sneaked in | None. `git diff ac608b5..1f3217e --name-only`: only `measurement/`, diary docs, `ci.yml`, `.gitignore`. Diff over `src/`, `en.toml`, `Cargo.toml`/`Cargo.lock` empty. |
| en.toml untouched | Confirmed (not in any M9 commit's file list). |

No new P1/P2 findings. No new P3 findings beyond the pre-existing note above.

## Mutation table

| # | Mutation | Target | Result |
| --- | --- | --- | --- |
| M1 | EOF branch disabled (`if False and answer_raw == ""`) | grade.py:121 | **CAUGHT** — both EOF pins fail fast (read-count guard), no hang |
| M2 | `save_grades` moved back after `grade_interactive` (no try/finally) | __main__.py:117 | **CAUGHT** — interrupt pin fails, sidecar empty |
| M3 | `_dedupe_by_node_id` → passthrough (DISTINCT kept) | pools.py:95 | **CAUGHT** — fan-out pin `assert 2 == 1` |
| M4 | unsure added to Wilson denominator | report.py:37 | **CAUGHT** — unsure-exclusion pin fails on interval values |

All mutations transient; tree reverted to clean `1f3217e` after each.

## Gates (run once, end of review)

* `cargo test --locked` (repo root) → **ok** (194 passed + 1 integration, 1 ignored).
* `pytest measurement/tests -q` → **39 passed**.
* `pytest ingester/tests -q` → **36 passed** (1 pre-existing warning).

## Verdict

**APPROVE — zero P1/P2 residue.** All three round-1 must-fixes hold under
independent mutation; the ten new pins are load-bearing (each targeted
mutation caught), deterministic across hash seeds, and scoped to the right
code path. Working tree left clean except this review document; nothing
committed.
