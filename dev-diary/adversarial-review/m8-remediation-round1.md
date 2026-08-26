# M8 round-1 remediation

Fixes every P1/P2 finding from `m8-round1.md` plus the flagged CI
reproducibility nit. Each behavioral pin below was mutation-tested: break the
fix → named test fails → restore → passes.

## P1-1 · CI collection failure — fixed

`pytest ingester/tests -q` from the repo root failed with
`ImportError: cannot import name 'walker' from 'ingester'`: pytest resolved
the repo-root `ingester/` directory as a namespace package instead of the
package inside it.

Fix (chosen option: path config, no install step): added
`pythonpath = ["."]` to `[tool.pytest.ini_options]` in
`ingester/pyproject.toml`. pytest discovers that ini file when invoked as
`pytest ingester/tests -q` (it is an ancestor config of the args), rootdir
becomes `ingester/`, and the rootdir-relative `.` puts `ingester/` on
`sys.path` before collection. Running from inside `ingester/` works unchanged
(cwd was already on `sys.path`; the setting is simply redundant there).

Verified all three shapes:

* `pytest ingester/tests -q` (repo root, exact ci.yml command) — **36 passed**
* `python -m pytest ingester/tests -q` (repo root) — **36 passed**
* `python -m pytest tests -q` (from `ingester/`) — **36 passed**

## P2-1 · Writer leaked the full parent environment — fixed

`writer.py::_build_params` passed `env=dict(os.environ)` into
`StdioServerParameters` (143 inherited vars in the canary experiment).
Replaced with `LamboMcpWriter._CHILD_ENV_ALLOWLIST`, a targeted allowlist of
exactly what the serve child needs:

* process essentials: `PATH`, `HOME`, `TMPDIR`, `LANG`, `TZ`
* store/embedder knobs: `LAMBO_STORE`, `LAMBO_EMBEDDER`, `LAMBO_EMBED_DIM`
* Gemini credentials: `LAMBO_GEMINI_PROJECT`, `LAMBO_GEMINI_LOCATION`,
  `LAMBO_GEMINI_CREDENTIALS`, `GCP_LAMBO_CREDENTIALS`,
  `GOOGLE_APPLICATION_CREDENTIALS`
* Postgres DSN authorities: `MOOSHIK_POSTGRES_DSN`, `LAMBO_POSTGRES_DSN`,
  `DATABASE_URL`

The list is derived from the documented proxy-path requirements
(`ingester/README.md` "Write path", M2 overlay names). Also dropped the
duplicate `import shlex`.

Pin: `test_writer_child_env_is_an_allowlist_not_wholesale_inheritance` plants
`MOOSHIK_VAULT_PASSPHRASE=canary`, `AWS_SESSION_TOKEN=canary` and a third
random secret, asserts all three are absent from the child env, that
`LAMBO_STORE`/`LAMBO_EMBEDDER`/`LAMBO_POSTGRES_DSN` pass through, and that
every key is within the allowlist.

**Mutation**: revert `_build_params` to `env=dict(os.environ)` → pin **fails**
(`'MOOSHIK_VAULT_PASSPHRASE' not in {...}` assertion error). Restored →
passes.

## P2-2 · Symlinked files escaped the corpus root — fixed

`walker.iter_files` now rejects any `path.is_symlink()` candidate during the
`rglob` walk (before the `is_file()` check, since `is_file()` follows links).
The corpus root is the trust boundary; a link crossing it is not followed.
Documented in the `iter_files` docstring.

Pin: `test_symlinks_never_cross_the_corpus_root_boundary` builds a root with
a regular `.md`, a file symlink pointing outside the root, and a directory
symlink pointing outside the root; only the real file may be collected.

**Mutation**: remove the `is_symlink()` guard → pin **fails** (outside
content collected via `link.md`). Restored → passes.

## P2-3 · At-least-once crash window undocumented — documented

Three places now state the semantics instead of claiming "never re-extract":

* `checkpoint.py` module docstring: new "Semantics are at-least-once"
  section — derive lands, crash before `checkpoint.mark` ⇒ next run
  re-extracts and re-writes the same concepts (**duplicates, never loss**);
  why that is acceptable for a bootstrap loader (memory graphs tolerate
  re-derives far better than gaps; M9-style curation can merge/retract,
  whereas a lost extraction needs a full corpus re-read); how to force a
  clean re-run (`rm <root>/.ingest/state.json` plus lambo-side retraction of
  previously written concepts if accumulation matters); corrupt-state
  behavior (`_load` swallows corruption and starts clean ⇒ same duplicate
  exposure as the crash window).
* `pipeline.py` module docstring + `ingest()` docstring: delivery order
  (derive + record-action first, mark last) and the resulting
  at-least-once guarantee.
* `ingester/README.md` §Provenance: operator-facing paragraph replacing the
  false "never re-extract" claim.

## CI determinism (flagged P3) — pip pins

Exact versions mirrored into both `ingester/pyproject.toml` and the ci.yml
install line (matching the versions the suite was developed and verified
against):

| package | pin |
|---|---|
| mcp | ==2.1.1 |
| google-genai | ==2.20.0 |
| google-adk | ==2.7.1 |
| pytest | ==9.1.1 |

The Dockerfile's `pip install .` inherits the pyproject pins automatically.

Other P3 notes (scanner regex breadth, walker dead-weight descent loop,
unreachable DROPPED branch, agent.py demo shape, marker-grep limits) are
documented here as accepted-for-now; no code change taken beyond the
one-line duplicate-import cleanup inside the already-touched writer.

## Gates

| Gate | Result |
|---|---|
| `cargo test --locked` (root) | **195 passed**, 0 failed, 1 ignored (pre-existing live GCP round trip) |
| `pytest ingester/tests -q` (repo root, exact ci.yml command, venv python 3.13) | **36 passed** |
| `python -m pytest tests -q` (from `ingester/`) | **36 passed** |

## Mutation summary

| Mutation | Pin | Result |
|---|---|---|
| `_build_params` → `dict(os.environ)` | env-canary allowlist pin | **CAUGHT** |
| drop `path.is_symlink()` guard in `iter_files` | symlink boundary pin | **CAUGHT** |

— M8 remediation, 2026-08-26
