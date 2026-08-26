# M8 adversarial review — round 2 (re-verification of the remediation)

Branch `m8-ingester`, HEAD `df8d779`. Independent round-2 trace and mutation of
every round-1 closure, plus a fresh-residue hunt. All mutations were transient
and reverted (`git status` clean between runs; final tree clean except this
file). Nothing was committed. Verdict up front: **APPROVE — zero P1/P2
residue**; three P3 notes recorded below.

## Closure-by-closure re-verification

### P1-1 · CI collection failure — VERIFIED FIXED

* Exact `ci.yml` command `pytest ingester/tests -q` from the repo root,
  `PYTHONPATH` unset, Python 3.13 venv holding exactly the pinned set:
  **36 passed**.
* `python -m pytest ingester/tests -q` (repo root): **36 passed**.
* `python -m pytest tests -q` (from `ingester/`): **36 passed**.
* No `conftest.py` exists anywhere in the repo (only pytest's own internals in
  `.venv`). The fix is the pure ini setting `pythonpath = ["."]` in
  `[tool.pytest.ini_options]` of `ingester/pyproject.toml`; nothing depends on
  an IDE or plugin, and the from-`ingester/` shape is unchanged.

### P2-1 · Child env wholesale inheritance — VERIFIED FIXED

`writer.py::_build_params` is a true **allowlist** (intersection of
`os.environ` with `_CHILD_ENV_ALLOWLIST`, 16 names) — not a denylist; it fails
closed: anything unlisted simply does not reach the child. Cross-checked every
name the child can actually read:

* lambo overlay (`src/config/overlay.rs` + lambo crate): `LAMBO_STORE`,
  `LAMBO_EMBEDDER`, `LAMBO_EMBED_DIM`, `LAMBO_GEMINI_PROJECT/_LOCATION/
  _CREDENTIALS` — all allowlisted.
* DSN chain `MOOSHIK_POSTGRES_DSN → LAMBO_POSTGRES_DSN → DATABASE_URL`
  (`overlay.rs:153-155`, lambo `FALLBACK_DSN_ENV`) — all three allowlisted.
* `memory/ops.rs:304-308`: `GCP_LAMBO_CREDENTIALS` with
  `GOOGLE_APPLICATION_CREDENTIALS` fallback — both allowlisted.
* README proxy-path export block exports exactly names that survive the
  filter; the README "Write path" list matches the tuple verbatim.

Credential probe (direct `_build_params` calls):

| Parent env | Child receives |
|---|---|
| only `GCP_LAMBO_CREDENTIALS=/sa/a.json` | `GCP_LAMBO_CREDENTIALS` ✓ (`GOOGLE_APPLICATION_CREDENTIALS` correctly absent — it is only the fallback, and passing it unset-in-parent would be fabrication) |
| README proxy block (`LAMBO_GEMINI_*` derived from `$GCP_LAMBO_CREDENTIALS`/`$MOOSHIK_GEMINI_*`, store/embedder knobs, DSN) | every exported var passes ✓ — live path intact |
| only `MOOSHIK_GEMINI_*` + `GOOGLE_APPLICATION_CREDENTIALS` | `GOOGLE_APPLICATION_CREDENTIALS` passes; `MOOSHIK_GEMINI_*` stripped — correct, the `lambo serve` binary never reads those names (they are mooshik-process overlay names); README documents exporting `LAMBO_GEMINI_*` for the child |

No needed variable is dropped on any documented path; the live proxy path used
in M8 (`LAMBO_GEMINI_CREDENTIALS`/`GCP_LAMBO_CREDENTIALS`) survives intact.

### P2-2 · Symlink escape — VERIFIED FIXED

The guard sits at `walker.py:80` — `if path.is_symlink(): continue` **before**
the `is_file()` check (which follows links), inside the `rglob` loop, so it
covers file symlinks *and* any yielded directory symlinks (rglob does not
recurse into dir symlinks anyway; the guard rejects them if seen). The pin
builds both shapes and asserts only the real file is collected.

### P2-3 · At-least-once window — VERIFIED DOCUMENTED

All three claimed sites carry the real semantics, checked by reading them:

* `checkpoint.py` module docstring: dedicated "Semantics are at-least-once"
  section (crash window ⇒ duplicates never loss; clean-run recipe; corrupt
  state degrades identically).
* `pipeline.py` module docstring lines 17-23: derive+record first, mark last,
  duplicates-not-loss, corrupt-state equivalence.
* `ingester/README.md` §Provenance: the false absolute claim is replaced by
  "Delivery is **at-least-once, not exactly-once** …" with operator recovery
  guidance and cross-references.

## Mutation table

Each mutation applied transiently, named pin run, then reverted (tree verified
clean after each).

| # | Mutation | Pin | Result |
|---|----------|-----|--------|
| R1 | Delete the entire `[tool.pytest.ini_options]` block (`pythonpath` line) from `ingester/pyproject.toml` | root-invocation collection itself | **CAUGHT** — `pytest ingester/tests -q` errors at collection (`Interrupted: 1 error during collection`, exit 2); from-`ingester/` shape still 36 passed |
| R2 | `_build_params` body → `env = dict(os.environ)` | `test_writer_child_env_is_an_allowlist_not_wholesale_inheritance` | **CAUGHT** — `'MOOSHIK_VAULT_PASSPHRASE' not in {…}` assertion failure |
| R3 | Remove the `path.is_symlink()` guard from `iter_files` | `test_symlinks_never_cross_the_corpus_root_boundary` | **CAUGHT** — outside content collected via `link.md` |

## New-residue hunt

* **Pin consistency**: `pyproject.toml` (`mcp==2.1.1`, `google-genai==2.20.0`,
  `google-adk==2.7.1`, dev `pytest==9.1.1`) matches the ci.yml install line
  name-for-name and version-for-version. Dockerfile `pip install .` inherits
  the pyproject pins by construction.
* **Does pinning break the Dockerfile build?** No. A fresh Python 3.13 venv +
  `pip install .` against these exact pins resolves cleanly (67 packages,
  `uv pip compile` exit 0), installs without conflict (`uv pip check`: "All
  installed packages are compatible"), imports fine, and the full offline
  suite passes **36/36** in that fresh environment. No google-adk dependency
  conflicts at these pins.
* **Walker skip logic vs legitimate deep files**: probed `root/a/b/c/deep.md`
  plus a `target/` dir and a file literally named `mytarget.md` — deep file
  and colliding-name file are collected; only paths with a skip-dir *path
  component* are filtered. No over-skipping beyond the documented SKIP_DIRS.
* **Suite counts coherent**: `--collect-only` = 36 = round-1's 34 + the two
  new pins (env allowlist, symlink boundary). Cargo side: `cargo test
  --locked` = 194 unit + 1 integration = **195 passed**, 1 ignored (the
  pre-existing live GCP round trip) — matches the remediation gate table.

## P3 notes (non-blocking, no action required this cycle)

1. `_CHILD_ENV_ALLOWLIST` omits `LAMBO_GEMINI_MODEL` /
   `MOOSHIK_GEMINI_MODEL`. An operator who pins a custom embedding model via
   env would find the hub-path serve child silently using the default model
   (dim stays consistent via `LAMBO_EMBED_DIM`). Not on any documented path;
   add two names if model overrides ever ship.
2. `checkpoint.py` line 1 still opens with "resume and never re-extract"
   without the "unchanged" qualifier. The dedicated at-least-once section four
   lines later corrects it explicitly, so this is cosmetic tension, not the
   false claim round 1 flagged.
3. The pre-existing stack-descent dead weight in `iter_files` (round-1 P3)
   remains, as accepted in the remediation doc.

## Gates

| Gate | Result |
|------|--------|
| `cargo fmt --check` (root) | clean |
| `cargo clippy --all-targets -- -D warnings` (root) | clean |
| `cargo test --locked` (root) | **195 passed**, 0 failed, 1 ignored |
| `pytest ingester/tests -q` (repo root, exact ci.yml command, `PYTHONPATH` unset, pinned venv py3.13) | **36 passed** |
| `python -m pytest ingester/tests -q` (repo root) | **36 passed** |
| `python -m pytest tests -q` (from `ingester/`) | **36 passed** |
| fresh `pip install .` venv (Dockerfile shape) → suite | **36 passed**, deps compatible |

## Verdict

**APPROVE — zero P1/P2 residue.** All four round-1 closures independently
reproduced, all three mutations caught by their named pins, no new blocking
residue found; three P3 notes recorded for the diary.

— M8 round-2 review, 2026-08-26
