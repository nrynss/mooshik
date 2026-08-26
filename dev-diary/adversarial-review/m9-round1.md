# M9 round 1 — adversarial review of the measurement harness

Reviewed at `d3999f4` (branch `m9-measurement`), worktree `/tmp/mooshik-m9`.
Scope: all of `measurement/*.py`, `tests/test_measurement.py`, the `ci.yml`
diff, cross-checked against `ingester/ingester/pipeline.py` / `writer.py`
(what M8 really wrote) and the pinned lambo crate at `f90a662` (what the
store really stores). Every claim below was executed, not read off.

## What held up under attack

* **Wilson implementation is correct.** Recomputed independently
  (`statistics.NormalDist().inv_cdf(0.975)` + closed form) for
  0/5 `[0.000000, 0.434482]`, 1/10 `[0.017876, 0.404150]`, 8/10
  `[0.490162, 0.943318]`, 27/27 `[0.875445, 1.000000]`, plus the harness'
  own live numbers 10/10 `[0.722467, 1.000000]` and 4/4
  `[0.510109, 1.000000]` — every value matches `stats.wilson` exactly.
  The diary's call against the brief's "8/10 → [0.579, 0.949]" is right:
  that pair matches no standard interval at z≈1.96.
* **Unsure exclusion is correct** at both levels: `precision()` takes only
  correct+incorrect; `PoolTally.interval` uses
  `wilson(correct, correct + incorrect)`; `tally` routes unsure to its own
  bucket. Excluded from numerator *and* denominator. (Report-level pin is
  missing — P3-6b below.)
* **Empty pools render n/a, not fake intervals**: `wilson(_, 0)` → None,
  `fmt_pct(None)` → "n/a", plus the "promotes nothing" callout when the
  canonical pool is empty.
* **Difference of precisions** is a point difference with both intervals
  shown separately and no unpooled-interval claim — honest framing
  (docstring states it; the report line itself could say so — P3-5).
* **Sampling determinism verified by execution**: two fresh runs against
  the fake seam at seed 42 produce **byte-identical** `sample.jsonl`;
  seed 7 differs. Per-pool streams are genuinely independent
  (`Random(f"{seed}:{pool}")`; same seed, different pools → different
  picks). `rng.sample(range(...))` is without-replacement; `min(n, len)`
  clamps; output sorted by candidate order so re-draw reproduces exactly.
* **Rejected-pool exclusion cannot leak overlap via ordering**: candidates
  are filtered by id set *before* the draw; the sampled indexes then map
  into the filtered list. Verified zero raw∩rejected overlap at full
  clamp (10 raw of 14, rejected request clamps to remaining 4).
* **Pool definitions match reality.** Lambo serializes statuses as the
  exact strings `'None' | 'Candidate' | 'Venerable' | 'Canonical'`
  (`canonization_status_sql`, `src/store/pg/mod.rs:1089`) — the column
  holds `'None'`, not SQL NULL, so `<> 'Canonical'` does not silently drop
  rows. Derive wires `Hierarchical` edges parent→child
  (`derive.rs`: ParentOf pairs become parent→child edges), and M8's
  pipeline writes `parent = document:<source>` with each concept content
  as child — `RAW_POOL_SQL` (edges target=concept, source content
  `LIKE 'document:%'`) is exactly that shape.
* **No SQL injection surface**: the only interpolated value is `%s`
  parameterized (session id); the `'%%'` escaping of the LIKE literal is
  correct for psycopg's paramstyle.
* **Coverage query cannot double-count**: plain `count(*)` /
  `count(embedding)` over one table, no joins (contrast P2-3).
* **Grade merge-on-save never drops verdicts**: `save_grades` loads what
  is persisted and updates over it; verified by test and by reading —
  grades for ids absent from a new sample survive.
* **TSV template is injection-safe**: content has tabs/newlines flattened
  to spaces on write; hostile-content probe produces one line and applies
  cleanly; unknown verdicts counted skipped; blank = ungraded.
* **Excerpt size bounded** at 700 chars, git path list-form subprocess
  with 10 s timeout, unresolvable refs render "(source not resolvable)"
  instead of pretending (the recreated-fixture SHA drift in the live run
  was handled exactly this way).
* **CI job is offline and consistently pinned**: same checkout /
  setup-python SHAs as the other jobs; installs the mirrored exact pins;
  no Rust, `en.toml`, or `src/` changes in either M9 commit (`git diff`
  over `src/` between `ac608b5..d3999f4` is empty).

## Findings

### P1

1. **Interactive grading hangs forever on EOF** (`grade.py:118`). When
   stdin closes (piped input exhausted, terminal dropped), `readline()`
   returns `""` forever — not `q`, not `s`, not in `KEYMAP` — so the loop
   reprompts infinitely and never reaches `save_grades`. Reproduced: a
   thread driving `grade_interactive` with an empty StringIO is still
   alive after 2 s, spinning on prompts. Fix: treat `""` (EOF) as quit,
   then save.

### P2

2. **Ctrl-C during interactive grading loses every verdict of the
   session** (`__main__.py:140`). `main()` catches `KeyboardInterrupt` →
   `sys.exit(130)`; the `save_grades` call in `_cmd_grade` sits *after*
   `grade_interactive` and never runs. A human who grades 13 of 14 items
   and hits Ctrl-C keeps none of them. Fix: save in a `finally` (or trap
   the interrupt inside the grading command).
3. **JOIN fan-out can duplicate pool rows, and nothing downstream dedups**
   (`pools.py:25-62`, `sample.py`). Raw/rejected pools join edges without
   `DISTINCT`; a concept with **two** `document:*` parents (the same
   content string extracted from two documents resolves to one node via
   lambo's content-keyed parent_of, gaining a second Hierarchical edge)
   yields two rows with the same `node_id`. Consequences, all verified
   against the fake seam: `rng.sample(range(len(candidates)))` can pick
   both indexes → the same node twice in one sample; `tally` counts it
   twice → inflated n/correct; pool sizes count rows not nodes. Not
   exercised by the live corpus (one parent each), but plausible on any
   corpus with repeated phrases across documents. Fix: `SELECT DISTINCT`
   on the node (or dedup by node_id in `draw`).

### P3

4. **Negative N crashes with a raw traceback**: `--raw -3` reaches
   `rng.sample(k=-3)` → `ValueError: Sample larger than population or is
   negative`, uncaught through `main()`. Validate N ≥ 0 in argparse.
5. **z constant is off by one ulp and the comment overclaims**:
   `Z_95 = 1.959963984540054` parses to a float 4.4e-16 away from
   `NormalDist().inv_cdf(0.975)` (= …536). Cosmetic at the report's three
   decimals, but the inline comment asserts equality. Also note: mutating
   z to exactly 1.96 survives the suite (±1e-3 tolerance absorbs it) —
   the values-pin is the real protection, the constant is decoration.
6. **Pin gaps found by mutation** (see below): (a) a fan-out join added
   to `COVERAGE_SQL` passes the whole suite — the marker-keyed fake seam
   validates cursor *reading*, not SQL *semantics*; (b) counting unsure
   in the interval denominator at report level passes too — no test
   renders an unsure verdict and asserts the denominator. Cheap pins:
   assert `"JOIN" not in COVERAGE_SQL` (+ dedup in `draw`), and a report
   test with mixed correct/unsure asserting `n graded`.
7. **Excerpt refs are unrestricted local reads** (hardening only):
   `document:file:<any absolute path>` reads any file's head into grading
   output, and a `document:git:<repo>#--output=<path>` style ref feeds
   attacker-chosen options to `git show` (list-form exec, no shell, but
   git options are reachable). Requires a corrupt/malicious graph row;
   worth a `--` separator before `<sha>` and a sanity check on refs.
8. **Report honesty nits**: the difference line doesn't say in the
   artifact itself that no interval is claimed on the difference; there
   is no explicit small-n caveat line (the wide intervals speak for
   themselves, but one sentence would match the diary's own caveat).
9. **Coverage denominator includes the `document:*` provenance resources
   themselves** (4 of 27 in the live run), slightly diluting the
   extraction-embedding signal the gate exists to protect.

## Mutation-tested pins

| Mutation | Result |
| --- | --- |
| Drop the `sqrt` term in `wilson` | **caught** — 3/6 Wilson tests fail |
| `Z_95` → `1.96` | survives (tolerance ±1e-3 absorbs ~1e-4 shift) |
| Sampling ordered-by-insertion (`range(min(n,len))`) | **caught** — `test_different_seed_draws_different_sample` fails |
| Fan-out join added to `COVERAGE_SQL` | **survives** — fake seam is blind to SQL semantics (P3-6a) |
| Unsure counted in interval denominator (`report.PoolTally.interval`) | **survives** — no unsure-rendering report test (P3-6b) |

All mutations were transient; tree reverted to `d3999f4` clean afterwards.

## Gates (run once, end of review)

* `cargo test --locked` (repo root) → **ok** (194 passed + 1 integration, 1 ignored).
* `pytest measurement/tests -q` → **29 passed**.
* `pytest ingester/tests -q` → **36 passed**.

## Verdict

**CHANGES REQUESTED** (round-1 remediation, three must-fix):

1. P1-1 EOF hang in interactive grading (quit + save on EOF).
2. P2-2 save grades on the interrupt path (`finally`).
3. P2-3 dedup pool draws by node id / `SELECT DISTINCT`.

Recommended alongside: P3-4 N validation and the two missing pins from
P3-6. Everything else — statistics, determinism, SQL/pool fidelity to the
lambo store, merge-on-save grading, coverage-first reporting — survived
adversarial recomputation and execution and needs no change.
