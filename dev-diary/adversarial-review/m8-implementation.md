# M8 implementation record — the bootstrap ingester

Scope, layout, decisions, and the live-verification log for M8. Branch
`m8-ingester`, worktree `/tmp/mooshik-m8`. Everything below was executed;
nothing is projected.

## Scope delivered

* `ingester/` Python project inside this repo (Decision 5): walker, secret
  scanner, chunker, checkpoint, Vertex extraction, MCP writer bridge, thin
  ADK-shaped agent module, CLI (`python3 -m ingester`).
* Ingest policy as spec'd: extension allowlist, secret hit **drops** the
  document, git repositories contribute **commit metadata only**.
* Checkpoint resume (`.ingest/state.json`, keyed `(source_path,
  content_hash)`), rate limiting with exponential 429 backoff.
* Provenance for M9: per-document Resource `document:<source>` produced by a
  `lambo_record_action`; every extracted concept wired as its child through
  `lambo_derive`'s `parent_of`.
* Offline pytest suite (34 tests, no network) + an `ingester` job in
  `.github/workflows/ci.yml`.
* `ingester/Dockerfile` + deploy section in `ingester/README.md` — **docs
  only; the deploy step itself is pending IAM setup** (service account with
  Vertex User + Cloud SQL Client, Artifact Registry writer). Recorded here
  explicitly as the brief requires.

## Layout

```
ingester/
  pyproject.toml          deps: mcp, google-genai, google-adk; dev: pytest
  README.md               usage, env table, decisions, Cloud Run deploy docs
  Dockerfile              python:3.13-slim, CMD python3 -m ingester
  ingester/
    config.py             env-driven Settings (INGEST_*, MOOSHIK_GEMINI_*)
    walker.py             allowlist walk + repo metadata-only rule
    secretscan.py         pattern classes + INGEST_EXTRA_FORBIDDEN vault list
    chunker.py            ~4k char budget, no overlap
    checkpoint.py         .ingest/state.json, atomic replace
    extraction.py         genai client factory + ConceptExtractor (retry/skip/backoff)
    writer.py             LamboMcpWriter over stdio_client
    pipeline.py           walk → scan → chunk → checkpoint → derive/action
    agent.py              ADK LlmAgent shape (see decision below)
    __main__.py           --root/--dry-run/-v CLI
  tests/test_ingest.py    34 offline seam tests
ingest-fixtures/          markdown corpus + fake-PEM drop case +
                          make-git-fixture.sh (2-commit fixture repo)
```

## Decisions taken

1. **ADK vs genai — direct `google-genai` for the batch loop; ADK shape kept
   in `agent.py`.** M8's loop is a deterministic map over chunks with
   checkpointing between calls; ADK's Runner/session abstractions are built
   for interactive multi-turn agents whose history they want to own, which
   we deliberately do not keep. `agent.py` defines a real runnable
   `LlmAgent` (same instruction, function tool wrapping the same writer
   seam), so the milestone's letter holds and a future interactive mode can
   reuse it.
2. **MCP client: official `mcp` package** (`stdio_client` +
   `ClientSession`). One sharp edge found live: the package filters the
   child environment to a whitelist by default, silently stripping
   `LAMBO_*`/DSN config from the subprocess — first live run wrote into a
   default SQLite graph because of exactly this. `writer.py` now passes the
   full environment explicitly.
3. **Provenance realization.** `lambo_derive` has no `produces` field on the
   wire (that is `lambo_record_action`). The brief's "source path in the
   derive produces resources" is realized as the pair: record_action
   produces `document:<source>`, derive wires every concept as its child via
   `parent_of`. Keeps lambo's `deny_unknown_fields` schemas honest.
4. **Repo walking is metadata-only by construction.** `git log
   --format='%H%x00%aI%x00%B%x1e'` cannot express a patch; directories
   holding `.git` are never descended for working-tree files. Pinned by a
   test that feeds a fixture repo and asserts no `diff --git`/`+++`/leaked
   content markers.
5. **Session naming.** Agent id `bootstrap` (configurable
   `INGEST_AGENT`); session defaults to `ingest-<hostname>`
   (`INGEST_SESSION`). For the proxy-path proof both sides ran session
   `mooshik` so the refused serve proxies into the mooshik holder.
6. **Model default `gemini-2.5-flash`.** `gemini-2.0-flash` returns 404 on
   this project/region (retired publisher model); probed live, 2.5-flash and
   2.5-flash-lite answer.

## Live verification (real Google endpoints, real Cloud SQL)

Environment: worktree `.env` (Cloud SQL DSNs, SA json paths, Vertex
project/location); lambo built at `/home/nryn/work/lambo/target/debug/lambo`
with `--features store-postgres,embed-gemini` (the default-feature binary
refuses postgres stores); mooshik built in the worktree.

Sequence (redacted outputs; no credential material shown):

1. **Holder up:** `./target/debug/mooshik serve` →
   `session endpoint bound … endpoint=/run/user/1000/lambo/mooshik-*.sock`
   (lease held and heartbeated: verified `expires_at` advancing in
   `session_leases`).
2. **Dry run:** `python3 -m ingester --root ../ingest-fixtures --dry-run`
   → `candidates : 5`,
   `dropped: file:…/credentials-backup.md [pem-block]` (path only, matched
   content never logged). No Vertex calls, no writes.
3. **Proxy-path write:** holder running, then the ingester with
   `INGEST_LAMBO_SERVE="…lambo serve --session mooshik"`. The child logs:
   > `lambo serve: proxying to the session holder (this process takes no
   > lease and holds no graph; every write happens in the holder…)`

   Report: `candidates : 5 · written : 4 · concepts : 14 · derive calls: 4 ·
   actions: 4 · chunks: 4`, dropped: credentials-backup.md [pem-block],
   exit=0. The two markdown files plus both fixture commits were extracted
   by gemini-2.5-flash on Vertex and written **through the J2 proxy into
   the holder's Cloud SQL graph**, agent id `bootstrap`.
4. **Fresh-process recall closes the loop.** Holder stopped, lease expired,
   then `./target/debug/mooshik recall 'Zephyr scheduler fairness quantum'`
   (new process, same Cloud SQL):
   ```
   1. The Zephyr scheduler assigns every task a fairness quantum of exactly
      40 milliseconds before a preemption check.   entity · relevance 3.89
   2. The Zephyr scheduler's fairness quantum was tuned on 2026-08-14 …
      observation · relevance 3.14
   4. Zephyr's message bus is called Windpipe …     entity · relevance 1.03
   5. The Windpipe ring never holds more than 512 in-flight messages.
      constraint · relevance 0.75
   ```
   Local files → Vertex inference → Cloud SQL store → Mooshik CLI: closed.
   Git-fixture concepts recalled too (`Cobalt Lantern serves weather data`
   etc., extracted from commit subjects/bodies only).
5. **Dropped-secret negative proof.**
   * Recall for the dropped document's distinctive marker
     `GRIMWAX-VAULT-ORCHID-7741`: no matching concept (only unrelated noise
     hits at relevance ≤ 0.54).
   * SQL over the same graph: `SELECT count(*) FROM concepts WHERE
     session_id='mooshik' AND (content LIKE '%GRIMWAX%' OR content LIKE
     '%PRIVATE KEY%' OR content LIKE '%AKIA%')` → **0** (of 27 total
     concepts, all `origin_agent='bootstrap'`).
   * Provenance visible in-graph: `document:file:/…/zephyr-architecture.md`,
     `document:git:/…/demo-repo#a7ade07…` Resources plus the
     `Ingested … N concepts extracted by gemini-2.5-flash` action records —
     M9's traceability hook works.
6. **Checkpoint resume live:** re-running the ingester over the same root →
   `written : 0 · resumed : 4`, zero derive/action calls, zero chunks.

## Tests

`pytest ingester/tests -q` → **34 passed**, fully offline (walker
allowlist/denylist & skip-dirs; every scanner pattern class plus the
vault-value list; chunker boundaries/no-overlap/hard-slice; checkpoint
round-trip/resume/corrupt-state; extraction parse-defensively/retry-once-
skip/429-backoff/non-429-raise; faked writer seam asserting derive payloads
carry valid types + provenance parent_of + produces resources; fixture git
repo pinned to emit no diff markers). CI gains an `ingester` job that
installs the deps and runs the same suite — seams only, no Google.

## Not in this cycle

* Cloud Run deploy execution — pending IAM setup (documented above).
* Anything touching the Rust build: `cargo test --locked` at the repo root
  stays green; the only non-`ingester/` changes are the `.github/workflows/
  ci.yml` ingester job and a `.ingest/` line in `.gitignore`.
