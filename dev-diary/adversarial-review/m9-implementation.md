# M9 implementation record — the measurement

Scope, layout, decisions, and the live-verification log for M9. Branch
`m9-measurement`, worktree `/tmp/mooshik-m9`. Everything below was executed
against the real Cloud SQL store; nothing is projected.

## Scope delivered

* `measurement/` Python subpackage (sibling of `ingester/`, same pyproject /
  pin / venv conventions) with CLI `python3 -m measurement {sample,grade,
  report}`.
* Seeded, deterministic sampling from the **live Cloud SQL graph** — no
  fabricated rows; every pool is a SQL query over what M7/M8 really wrote.
* Human-in-the-loop grading keyed by node id, interactive or editor/file mode.
* Markdown report: embedding coverage FIRST behind a 90% warning gate, then
  raw-extraction precision, canonical-fact precision, their difference, and
  the wrongly-rejected rate — all with Wilson score intervals (95%).
* Offline pytest suite: **29 tests**, zero network. SQL access sits behind a
  one-method `Connection` seam (`measurement/db.py`) that tests fake with a
  marker-keyed cursor. psycopg is imported lazily inside `PgConnection`, so
  CI needs only pytest.
* `.github/workflows/ci.yml`: `measurement` job mirroring the ingester
  offline job (`pytest measurement/tests -q`).
* `cargo test --locked` at repo root untouched and green.

## Layout

```
measurement/
  pyproject.toml            psycopg[binary]==3.2.13 pinned; dev: pytest==9.1.1
  measurement/
    db.py                   Connection protocol + PgConnection + DSN-from-env
    pools.py                pool SQL: raw / canonical / rejected / coverage
    sample.py               per-pool seeded RNG streams, jsonl io
    grade.py                grades sidecar, template emit/apply, interactive
    excerpt.py              document:<ref> -> source excerpt resolution
    report.py               markdown rendering + coverage gate
    stats.py                Wilson interval (z=1.959963984540054)
    __main__.py             argparse CLI
  tests/test_measurement.py 29 offline tests (fake seam, no network)
```

## Decisions taken

1. **Pool definitions** (all in `pools.py`, tied to what the pipelines wrote):
   * *raw pool* — targets of `Hierarchical` edges whose source concept's
     content starts with `document:` — exactly the provenance wiring M8's
     ingester produced (`lambo_record_action` produces `document:<source>`,
     `lambo_derive`'s `parent_of` wires each extracted concept as its child;
     edge direction source=document → target=concept).
   * *canonical pool* — `canonization_status = 'Canonical'`. The enum was read
     from the pinned lambo crate, `src/types/mod.rs`:
     `None | Candidate | Venerable | Canonical`.
   * *wrong-rejection pool* — the non-canonical slice of the raw pool
     (status `None`/`Candidate`/`Venerable`). Lambo has no explicit
     `Rejected` status: budget demotions land on `None`, unevaluated
     concepts sit at `None`/`Candidate`/`Venerable`. Every extracted fact
     that is not Canonical today was implicitly not promoted, so this is the
     gradeable wrong-rejection population.
2. **Determinism.** Each pool draws from its own `random.Random(f"{seed}:
   {pool}")`, so adding `--rejected N` later never perturbs an earlier raw or
   canonical draw; pools are ordered by node id in SQL before drawing;
   `rng.sample` gives without-replacement draws (no duplicates within a pool);
   `N > pool size` clamps. The rejected draw excludes node ids already drawn
   into the raw/canonical samples so no item is graded twice. Verified live:
   same seed → byte-identical sample file; seed 42 vs 7 → different draws.
3. **Interval method.** Wilson score interval at z = 1.959963984540054
   (two-sided 95%), no continuity correction: it stays inside [0,1] where
   Wald does not (0/10 Wald reads [0,0]) and behaves at small n. `unsure`
   verdicts are excluded from numerator and denominator; `n/a` for empty
   populations rather than a fake interval. Difference of precisions is
   reported as a point difference of the two estimates with both intervals
   shown (no unpooled-interval claim). **Note:** the brief's illustrative
   "8/10 → [0.579, 0.949]" matches no standard interval at z=1.96 (checked
   against Wilson, continuity-corrected Wilson, Agresti-Coull, Jeffreys,
   Clopper-Pearson); the true closed-form value is [0.4902, 0.9433] and the
   tests pin that.
4. **Grading UX.** Grades persist to a jsonl sidecar (default
   `<sample-stem>.grades.jsonl`) keyed by node id; `save_grades` merges into
   what is already persisted, so re-sampling never drops verdicts for items
   absent from the new draw. Editor mode: `--template F` writes a TSV of
   `node_id⇥verdict⇥pool⇥content` (verdict blank), fill column 2 with
   correct/incorrect/unsure, then `--apply F`. Interactive mode prints each
   ungraded item with its source excerpt and reads c/i/u/s/q keystrokes.
5. **Excerpt resolution** (`excerpt.py`): `document:file:<path>` reads the
   file head; `document:git:<path>#<sha>` runs `git -C <path> show --no-patch
   --format=fuller <sha>` (commit metadata is all the ingester ever
   extracted). Unresolvable refs render "(source not resolvable …)" instead
   of pretending.

## Live verification (real Cloud SQL)

Environment: worktree `.env` `MOOSHIK_POSTGRES_DSN`; sources re-materialized
at their recorded paths by copying `ingest-fixtures/` to
`/tmp/mooshik-m8/ingest-fixtures/` and running `make-git-fixture.sh`.

Graph state found (session `mooshik`, written by M8's live run): **27
concepts, 16 with a durable embedding (59.3%)**, statuses `{None: 17,
Candidate: 10}` split Entity 13 / Resource 6 / Logic 4 / Observation 3 /
Constraint 1; **canonical pool size: 0**; raw pool size: 14; rejected pool
size: 14. Four `document:*` provenance concepts present (2 files, 2 git
commits) with the `Ingested … N concepts extracted by gemini-2.5-flash`
action records.

Sequence:

1. `sample --raw 10 --canonical 5 --rejected 5 --seed 42 --session mooshik`
   → `sampled 14 of (raw=14, canonical=0, rejected=14)` — canonical request
   clamps honestly to the empty pool; rejected draws 4 after excluding the
   10 raw ids (the exclusion rule, working as designed, leaves 4 candidates).
2. Determinism re-run with the same seed → `cmp` byte-identical files.
   Seed 7 → different draws.
3. Grading: `--template grades.tsv`, hand-filled all 14 verdicts against the
   fixture sources (`team-handbook.md`, `zephyr-architecture.md`, and the two
   commit messages pinned verbatim in `make-git-fixture.sh`), then
   `--apply` → `applied 14, skipped 0`. Every sampled concept matches its
   source text in substance, including the vacuous-but-true bare noun
   extractions ("Cobalt Lantern", "Weather data") which are graded on truth,
   not informativeness. One honest caveat recorded here: the recreated git
   fixture gets fresh commit SHAs (commit timestamps differ), so the two
   `document:git:#<sha>` refs resolve to "source not resolvable" — those four
   items were graded manually against the commit-message text preserved in
   `make-git-fixture.sh`, which is character-identical to what M8 ingested.
4. `report` (markdown above in full):

| population | n graded | correct | precision | Wilson 95% |
|---|---|---|---|---|
| raw-extraction | 10 | 10 | 100.0% | [0.722, 1.000] |
| canonical-fact | 0 | 0 | n/a | n/a |
| wrongly-rejected rate | 4 | 4 | 100.0% | [0.510, 1.000] |

   Embedding coverage reported first: **16/27 (59.3%)**, below the 90% gate,
   so the report carries the explicit keyword-leg-recall warning.

### Interval math spot-check (hand recomputation)

Wrongly-rejected rate, k = 4 correct of n = 4 graded, z = 1.959963984540054:

```
p      = 4/4                       = 1
denom  = 1 + z²/n                  = 1 + 3.84138…/4        = 1.96036470…
center = (p + z²/2n)/denom         = (1 + 0.480172…)/1.96036 = 0.75505458…
half   = z·√(p(1−p)/n + z²/4n²)/denom
       = z·(z/8)/1.96036           = 0.480172…/1.96036     = 0.24494541…
lower  = center − half             = 0.51010916…
upper  = min(1, center + half)     = 1
```

→ **[0.510, 1.000]**, matching the harness output exactly.

## Reading of the result

The corpus is too small for tight intervals (raw lower bound 72%), but the
structural finding is exactly the one the milestone brief predicted: the
canonical pool is **empty** — canonization promoted nothing on this corpus,
so canonical-fact precision is undefined (a filter promoting nothing has
trivially "perfect" precision), and the only live signal about canonization
quality is the wrong-rejection rate: 4 of 4 sampled non-promoted facts are
true per their sources (100%, Wilson [0.510, 1.000]). Nothing was wrongly
rejected among the graded sample because *everything true was rejected*.
And with durable-embedding coverage at 59.3%, recall over this graph runs on
the keyword leg alone — any future recall number must clear the coverage
gate first.

## Tests and gates

* `pytest measurement/tests -q` → **29 passed** (offline; fake connection
  seam covers seeded determinism, clamping, duplicate-free draws, grade
  persistence across re-sampling, template round-trip, Wilson known values
  including 8/10 ≈ [0.4902, 0.9433], 0/10 and 10/10 edges, n=0 → None,
  coverage-cursor logic, report ordering/gating/difference reporting,
  excerpt resolution).
* `pytest ingester/tests -q` → 36 passed (untouched).
* `cargo test --locked` at repo root → green (Rust tree untouched).

Commits: `feat(measurement)` harness + tests + CI job; `docs(diary)` this
record. No push, per the brief.
