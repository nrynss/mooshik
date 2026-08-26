# M9 round-1 remediation

Fixes every must-fix from `m9-round1.md` plus the recommended cheap P3s.
Base: `d3999f4`, branch `m9-measurement`.

## Per-finding fixes

### P1-1 — EOF hang in interactive grading (`grade.py`)

`grade_interactive` now reads the raw line first and treats `""` (EOF) as
quit — identical to `q` — so a closed stdin (piped input exhausted, dropped
terminal) exits the loop and reaches `save_grades` instead of reprompting
forever. A blank typed line still reprompts (it is `"\n"`, not `""`).

Pin: `test_eof_quits_instead_of_reprompting_forever` feeds an always-EOF
stream with a read-count guard (fails fast at 64 reads instead of hanging)
and asserts exactly one prompt then return;
`test_eof_via_cli_still_persists_grades` drives `_cmd_grade` through
monkeypatched `sys.stdin` (one verdict, then EOF) and asserts the sidecar.

### P2-2 — Ctrl-C lost all verdicts of a session (`__main__.py`)

`_cmd_grade` wraps the interactive loop in `try/finally`; `save_grades`
runs even when `KeyboardInterrupt` propagates (main() then exits 130).

Pin: `test_keyboard_interrupt_mid_session_still_persists` raises the
interrupt from the input fake after two verdicts and asserts both reached
the sidecar file.

### P2-3 — JOIN fan-out duplicated pool rows (`pools.py`)

Two layers:

1. `RAW_POOL_SQL` / `REJECTED_POOL_SQL` now `SELECT DISTINCT`.
2. `raw_pool` / `rejected_pool` pass results through
   `_dedupe_by_node_id` (first occurrence per node_id wins; order stable
   because SQL orders by id). DISTINCT alone cannot collapse two rows that
   differ in `source_ref` — the same content extracted from two documents
   gains a second Hierarchical edge and exactly that row shape.

Deduping at the pool seam fixes every consumer at once: sample draws,
pool-size counts (`run_sampling` and the report's sizes) and tallies.

Pins: `test_join_fanout_duplicate_node_drawn_and_counted_once` (two rows,
same node_id, different source_ref → drawn once, size 1, tally sees one);
`test_coverage_sql_is_join_free` (structural guard keeping `COVERAGE_SQL`
a single-table count, per review mutation P3-6a).

### Missing pin from P3-6b — unsure excluded at report level

`test_unsure_excluded_from_report_n_and_interval` renders a 3-item raw
sample graded 2×correct + 1×unsure and asserts the table row shows
`n graded = 2` with the interval equal to `wilson(2, 2)` formatted to the
report's three decimals. The review's mutation (counting unsure in the
interval denominator) changes both numbers and fails this pin.

### P3-4 — negative N argparse validation (`__main__.py`)

`_non_negative_int` type on `--raw/--canonical/--rejected` raises
`ArgumentTypeError` → clean usage message, exit code 2.

Pins: parametrized negative-N test asserts `SystemExit` code 2 for all
three flags; zero stays accepted.

### P3-5 — z-constant comment overclaim (`stats.py`)

Comment now states agreement "within ~5e-16" with
`NormalDist().inv_cdf(0.975)` instead of implying bit equality.

## Mutation verification

| Mutation | Result |
| --- | --- |
| P1-1: EOF branch disabled (`if answer_raw == ""` → `if False`) | **caught** — both EOF pins fail via the read-count guard (`grading kept prompting after input ended`), fast, no hang |
| P2-3: `SELECT DISTINCT` dropped + `_dedupe_by_node_id` returns rows unchanged | **caught** — fan-out pin fails (`assert 2 == 1` on duplicate node count); suite otherwise green |

Both mutations transient; tree restored and re-verified green afterwards.

## Gates

* `cargo test --locked` (repo root) → **ok** (194 passed + 1 integration, 1 ignored).
* `pytest measurement/tests -q` → **39 passed** (29 before, +10 pins).
* `pytest ingester/tests -q` → **36 passed** (1 warning, pre-existing).
