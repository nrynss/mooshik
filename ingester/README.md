# Mooshik bootstrap ingester (M8)

A batch pipeline that walks a corpus root on this machine, has **Gemini
Flash** extract memory concepts at volume on **Vertex AI**, and writes them
into **Mooshik's Cloud SQL Postgres graph** through `lambo serve` over MCP.

Google is the store *and* the inference: extraction runs on Vertex Gemini
Flash; writes land in the same Cloud SQL store the Rust side reads. No local
model, no SQLite.

## Usage

```bash
cd ingester
python3 -m venv .venv && . .venv/bin/activate
pip install -e ".[dev]"        # or: pip install mcp google-genai google-adk pytest

# credentials (worktree .env): MOOSHIK_GEMINI_PROJECT / _LOCATION /
# _CREDENTIALS for Vertex; MOOSHIK_POSTGRES_DSN / LAMBO_POSTGRES_DSN for the
# lambo/mooshik processes themselves.
set -a && source ../.env && set +a

python3 -m ingester --root ../ingest-fixtures --dry-run   # report phase
python3 -m ingester --root ../ingest-fixtures             # real write

pytest -q                                                  # offline seam tests
```

### Environment

| Variable | Default | Meaning |
| --- | --- | --- |
| `INGEST_EXTENSIONS` | `.md,.markdown,.txt,.rst` | extension **allowlist** (never a denylist) |
| `INGEST_EXTRA_FORBIDDEN` | *(empty)* | comma-separated secret values to drop on sight — paste current `mooshik secret list` values here; vault values never leave the Rust process |
| `INGEST_LAMBO_SERVE` | `lambo serve` | writer command, e.g. `/path/to/lambo serve` |
| `INGEST_SESSION` | `ingest-<hostname>` | session id for the MCP writes |
| `INGEST_AGENT` | `bootstrap` | agent id recorded in the graph |
| `INGEST_MODEL` | `gemini-2.5-flash` | Vertex extraction model |
| `INGEST_CHUNK_CHARS` | `4000` | chunk budget (overlap disabled) |
| `INGEST_SLEEP_SECS` | `0.5` | sleep between Vertex calls |
| `INGEST_MAX_ATTEMPTS` | `4` | attempts before a 429 gives up (exponential backoff) |
| `INGEST_STATE` | `.ingest/state.json` | checkpoint file (relative paths resolve against `--root`) |
| `MOOSHIK_GEMINI_PROJECT` / `_LOCATION` / `_CREDENTIALS` | — | Vertex project, region, service-account json |

## Ingest policy (non-negotiable)

1. **Allowlist by extension.** Only listed extensions are candidates.
2. **Secret scanner drops documents.** PEM blocks, `AKIA…`, `ghp_…`/
   `github_pat_…`, Slack `xox…`, generic high-entropy assignments
   (`SECRET|TOKEN|PASSWORD|API_KEY = long literal`), plus every value in
   `INGEST_EXTRA_FORBIDDEN`. A single hit drops the whole document — never
   redaction. Logs carry the path and pattern-class name only.
3. **No diff content.** When the walk reaches a git repository it takes
   commit metadata only (`git log`: hash, author date, subject + body) and
   does not descend into the working tree. The log format cannot express a
   patch, so rule 3 holds by construction.

## Write path

The ingester speaks MCP over stdio to a **`lambo serve` subprocess**
(`INGEST_LAMBO_SERVE`). Lambo's J2 makes a refused `serve` proxy into the
session holder when one exists (Mooshik chat holding the lease), and become
the hub otherwise — either way exactly one graph gets written and the
companion stays up while the corpus loads.

Tools called: `lambo_derive`, `lambo_record_action`, `lambo_recall`, with the
same parameter shapes as `src/tools/schema.rs`.

Client choice: the official **`mcp` package** (`stdio_client` +
`ClientSession`). It matched lambo's wire shapes with zero friction, so no
hand-rolled JSON-RPC was needed. One sharp edge, fixed in `writer.py`: the
package filters the child environment to a whitelist by default, which
silently strips `LAMBO_*`/DSN config from the subprocess — the writer
therefore inherits the full environment explicitly.

For the **proxy path** the `lambo serve` child must resolve the same store
as the holder, so a refused start proxies into it instead of opening its own
(SQLite) graph. With a feature-built binary
(`cargo build --features store-postgres,embed-gemini`) export before the
run — the writer passes its whole environment to the child:

```bash
export LAMBO_STORE=postgres LAMBO_EMBEDDER=gemini LAMBO_EMBED_DIM=1536
export LAMBO_GEMINI_CREDENTIALS="$GCP_LAMBO_CREDENTIALS" \
       LAMBO_GEMINI_PROJECT="$MOOSHIK_GEMINI_PROJECT" \
       LAMBO_GEMINI_LOCATION="$MOOSHIK_GEMINI_LOCATION"
export INGEST_LAMBO_SERVE="/path/to/lambo serve --session mooshik"
export INGEST_SESSION=mooshik   # must match the holder's session id
```

A healthy proxy logs `proxying to the session holder … takes no lease`;
writes then land in the holder's graph under the ingester's agent id.

## Provenance (for M9)

Every document becomes a graph Resource named `document:<source>`:

* a `lambo_record_action` per document *produces* that resource,
* every extracted concept is wired as its **child** via `lambo_derive`'s
  `parent_of`.

So any concept traces back to its source file (or `git:<repo>#<sha>`)
in one hop. Note: `lambo_derive` has no `produces` field on the wire — the
brief's "source path in the derive produces resources" is realized as this
record-action + parent_of pair, which keeps lambo's schemas honest.

Checkpoint state lives in `.ingest/state.json` keyed by
`(source_path, content_hash)`: re-runs resume and never re-extract unchanged
documents; changed content re-extracts under its own key.

## ADK vs genai (decision)

The milestone asks for an ADK agent. `ingester/agent.py` is that ADK shape:
an `LlmAgent` ("given a chunk of text, return concepts") whose function tool
routes through the same writer seam. But the batch pipeline drives
**`google-genai` directly**: M8's loop is a deterministic map over chunks
with checkpointing and rate limiting between calls, and ADK's Runner/session
abstractions are built for interactive multi-turn agents whose history they
want to own — we deliberately keep none. Documented here and in
`dev-diary/adversarial-review/m8-implementation.md`.

## Deployment (Cloud Run) — docs only, deploy step pending IAM setup

The container builds and runs; the deploy step itself is pending IAM setup
(service account with Vertex User + Cloud SQL Client roles, Artifact
Registry writer for CI). Recorded as such for this cycle.

Build and push once IAM exists:

```bash
gcloud artifacts repositories create mooshik \
  --repository-format=docker --location=us-central1

docker build -t us-central1-docker.pkg.dev/PROJECT/mooshik/ingester:latest .
docker push us-central1-docker.pkg.dev/PROJECT/mooshik/ingester:latest
```

Deploy with Cloud Run **VPC egress / Unix socket for Cloud SQL**, mounting
the ingester service account:

```bash
gcloud run deploy mooshik-ingester \
  --image us-central1-docker.pkg.dev/PROJECT/mooshik/ingester:latest \
  --region us-central1 \
  --service-account INGESTER_SA@PROJECT.iam.gserviceaccount.com \
  --set-env-vars MOOSHIK_GEMINI_PROJECT=PROJECT,MOOSHIK_GEMINI_LOCATION=us-central1,\
MOOSHIK_GEMINI_CREDENTIALS=/secrets/sa.json,INGEST_SESSION=ingest-cloudrun \
  --no-allow-unauthenticated
```

Environment list (all from the table above): `MOOSHIK_GEMINI_*`,
`INGEST_*`, and the Postgres DSN passed to the `lambo serve` child through
lambo's own env chain. Service-account note: the runtime SA needs
`roles/aiplatform.user` (Vertex), `roles/cloudsql.client` (Cloud SQL), and
its json referenced by `MOOSHIK_GEMINI_CREDENTIALS` — or Application Default
Credentials when running on GCP, in which case leave
`MOOSHIK_GEMINI_CREDENTIALS` unset.
