# M8 adversarial review — round 1

Branch `m8-ingester`, HEAD `5acc642`. Reviewer ran every experiment below live
against the worktree; all mutations were transient and reverted (tree clean
except this file). Verdict up front: **approve with one blocking CI defect**
— the code is genuinely solid on the non-negotiables, but the committed CI
job cannot pass as written.

## P1 — findings that block

### P1-1 · The `ingester` CI job fails at collection

`ci.yml` runs `pytest ingester/tests -q` from the repo root with only
`pytest mcp google-genai google-adk` installed — the local package is never
installed (`pip install -e .` is absent) and there is no conftest.py or
`pythonpath` ini setting. The test module's `from ingester import walker`
then resolves against the repo-root `ingester/` *directory* (a namespace
package), not the package inside it:

```
ImportError: cannot import name 'walker' from 'ingester' (unknown location)
ERROR ingester/tests/test_ingest.py
```

Reproduced locally under Python 3.14 + current pytest; nothing about the
ubuntu-latest runner changes this sys.path arithmetic. The suite passes only
as `cd ingester && python -m pytest tests -q` (cwd lands on sys.path). The
implementation log's "34 passed" is true of that invocation, but the
committed gate is red. Fix options (any one): a `conftest.py` at
`ingester/`, `[tool.pytest.ini_options] pythonpath = ["."]`, or
`pip install -e .` in the job.

## P2 — should fix before this is load-bearing

### P2-1 · Writer hands the entire parent environment to the child

`writer.py::_build_params` passes `env=dict(os.environ)` into
`StdioServerParameters`. Demonstrated with canaries: a parent env containing
`MOOSHIK_VAULT_PASSPHRASE=CANARY-…` and `AWS_SESSION_TOKEN=…` yields both,
verbatim, in the child environment (143 vars inherited). The README frames
this as necessary because the `mcp` package's default whitelist strips
`LAMBO_*`/DSN config — true — but the correct response to an over-narrow
default whitelist is a targeted allowlist (`LAMBO_*`, store/DSN/GCP vars),
not wholesale inheritance. As written, every secret in the ingester's env
rides into a subprocess whose own logs/config surface is outside our
control.

### P2-2 · Symlinked files escape the corpus root

`iter_files` collects via `root.rglob("*")`, which follows file symlinks
(directory symlinks are correctly not recursed — verified: a symlinked dir
pointing outside contributed nothing). A `link.md → /elsewhere/x.md` inside
the root was collected with its outside content intact. The scanner still
applies to whatever comes in, so the drop rule holds, but the root boundary
is violated: any readable `.md` on the machine reachable by a symlink gets
ingested. One-line fix shape: reject `path.is_symlink()` candidates.

### P2-3 · Crash window between derive and checkpoint duplicates concepts

Demonstrated end-to-end with a writer whose `record_action` raises after
`lambo_derive` has landed: run 1 writes concepts into the graph then dies;
run 2 resumes (checkpoint was never marked) and derives the same concepts
again. Semantics are **at-least-once** — duplicates, never loss — which is
the right direction for memory, but the tradeoff is documented nowhere
(README §Write path and checkpoint docstring both say "never re-extract",
which is false across this window, and corrupt-state recovery has the same
effect: `_load` swallows corruption and re-ingests everything). Worth one
paragraph in README plus a M9 heads-up; dedup-on-write would be lambo-side
work.

## P3 — notes and nits

* **Scanner false negatives** (all demonstrated): base64-encoded PEM bodies
  undetected (no decode step); lowercase `-----begin …-----` header
  undetected (regex is uppercase-only); vault-value matching is case-
  sensitive exact substring (`Zephyrbreeze-42` misses value
  `ZephyrBreeze-42`); joined key names escape the `\b` boundary
  (`ACCESS_TOKEN = …`, `MYSECRET = …` don't match generic-assignment);
  values under 20 chars escape it too. No catastrophic backtracking: the
  patterns are single-class linear scans — a 7 MB near-miss line scans in
  64 ms.
* **Scan order is correct**: `plan()` scans each full `Document.text` before
  any chunking, so a PEM split across chunk boundaries cannot slip through
  (chunk N ends mid-header ⇒ still caught, since the whole text is scanned).
  Binary content in an allowlisted extension is read with
  `errors="replace"`; ASCII secrets survive replacement, so no bypass there.
* **Path-only logging verified**: the drop log line carries source path +
  pattern class name only; no exception path prints matched content
  (`read_text(errors="replace")` means no decode traceback either).
* **The metadata-only pin greps markers, but messages may legally contain
  them**: a commit whose message body includes literal `diff --git / ---
  / @@ / -TOP SECRET` lines emits exactly those markers through `%B`
  (demonstrated). That is message content, not patch leakage — `%B` cannot
  express a patch — but it means the marker-grep test proves less than the
  log claims, and a repo with such a commit message would fail the pin
  without any leak having occurred. Also: commit bodies containing `\x1e`
  split records and get silently dropped by the malformed-record guard.
  Non-UTF-8 commit messages survive (git re-encodes; verified).
* **Walker dead weight**: the stack-descent loop at the top of `iter_files`
  computes nothing (files come solely from the later `rglob`); its `.git`
  early-skip is duplicated by `_in_repo`. Duplicate `import shlex` in
  `writer.py`.
* **Checkpoint DROPPED state is unreachable**: dropped documents are never
  marked into the state file (only kept docs reach `checkpoint.mark`), so
  the `previous == DROPPED: continue` branch in `ingest()` can never fire.
  Harmless (rescanning is cheap) but dead machinery implying a guarantee
  that doesn't exist.
* **agent.py demo-shape**: hardcodes `"bootstrap"` instead of
  `settings.agent_id`; `record_concepts` assumes payload keys (KeyError on
  malformed model JSON); module-global writer. Fine for the letter-of-ADK
  role; would need hardening if an interactive mode ever ships.
* **Extraction injection bounds hold**: hostile chunks can at worst produce
  junk *concepts*, bounded by enum validation (unknown `concept_type`
  dropped — 100 injected entries yielded 0), flood cap 64/chunk, content
  clamped at 16 384 chars, parse-retry-once-then-skip verified, JSON errors
  caught (JSONDecodeError ⊂ ValueError). No dedup against existing graph
  concepts — junk persists until M9-style curation.
* **CI reproducibility**: actions SHAs pinned (checkout v7.0.1, setup-python
  v5 — SHA↔tag mapping not verifiable offline); pip deps float
  (`mcp>=1.2`, `google-genai>=1.0`, `google-adk>=1.0`) — unpinned minors,
  non-reproducible installs. Tests themselves hit no network (fakes + local
  git fixture only).
* **Live-log coherence**: per-run numbers cohere exactly — candidates 5 =
  3 markdown files + 2 fixture commits; dropped 1 ⇒ written 4; chunks 4 =
  one ≤4k-char chunk each; derive calls 4 = actions 4; resumed 4 on rerun;
  recall hits quote real fixture content. One gap: the negative-proof SQL
  counts **27** bootstrap-origin concepts in session `mooshik` where the
  logged successful write produced **14** — +13 unaccounted. Probably an
  earlier partial run after the env fix, but the record should say so or
  the arithmetic reads as noise.

## Mutation-tested pins

Each mutation applied transiently, suite run, then reverted (`git status`
clean between runs).

| # | Mutation | Pins killed |
|---|----------|-------------|
| M1 | PEM pattern disabled in `secretscan._PATTERNS` | `test_pattern_classes[pem-block ×2]`, `test_secret_hit_drops_whole_document_before_any_write` — **3 failed** |
| M2 | `_in_repo` returns False (walker descends repo working trees) | `test_files_inside_a_repo_are_not_walked_only_its_metadata` — **1 failed** |
| M3 | Checkpoint key drops the content hash | `test_changed_content_gets_a_new_key` — **1 failed** |
| M4 | Pipeline strips `parent_of` from the derive call | `test_derive_payloads_carry_provenance_and_valid_types` — **1 failed** |

All four pins are load-bearing: each mutation is caught immediately.

## Gates

| Gate | Result |
|------|--------|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test` | **194 passed**, 1 ignored, 0 failed |
| `pytest ingester/tests -q` (repo root, CI invocation) | **collection error** — see P1-1 |
| `python -m pytest tests -q` (from `ingester/`) | **34 passed**, 1 upstream DeprecationWarning |

## Verdict

Approve the implementation direction — scan-before-chunk, metadata-only git
walking, hash-keyed resume, schema-bounded extraction, and provenance wiring
are all real and mutation-pinned. **Do not merge the branch as-is**: P1-1
means the advertised offline CI gate does not exist yet (one-line fix).
P2-1/P2-2 are small diffs with outsized security posture value and should go
in before the first Cloud Run deploy; P2-3 needs documentation at minimum.
