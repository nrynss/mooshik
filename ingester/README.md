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
| `INGEST_LAMBO_SERVE` | `mooshik serve` | writer command; a raw `lambo serve` also works |
| `INGEST_SESSION` | `ingest-<hostname>` | session id for the MCP writes |
| `INGEST_AGENT` | `bootstrap` | agent id recorded in the graph |
| `INGEST_MODEL` | `gemini-3.7-flash` | Vertex extraction model |
| `INGEST_LOCATION` | `global` | Vertex region for extraction. **Not** `MOOSHIK_GEMINI_LOCATION` — see below |
| `INGEST_CHUNK_CHARS` | `4000` | chunk budget (overlap disabled) |
| `INGEST_SLEEP_SECS` | `0.5` | sleep between Vertex calls |
| `INGEST_MAX_ATTEMPTS` | `4` | attempts before a 429 gives up (exponential backoff) |
| `INGEST_STATE` | `.ingest/state.json` | checkpoint file (relative paths resolve against `--root`) |
| `MOOSHIK_GEMINI_PROJECT` / `_LOCATION` / `_CREDENTIALS` | — | Vertex project, region, service-account json. `_LOCATION` is the **embedder's** region and is not used for extraction |

### Two models, two regions

Extraction and embedding do not live in the same place, and the ingester keeps
them apart on purpose.

* **Extraction** runs `gemini-3.7-flash` at `INGEST_LOCATION=global`. Every
  Gemini 3.x flash model is served from `global` only — requesting one in
  `us-central1` returns `404 NOT_FOUND: Publisher model ... was not found or
  your project does not have access to it`, verified live 2026-08-31 for
  `gemini-3.5-flash`, `gemini-3.6-flash` and `gemini-3.7-flash`.
* **Embedding** runs `gemini-embedding-001` at `MOOSHIK_GEMINI_LOCATION`
  (`us-central1`), which the deploy also maps to `LAMBO_GEMINI_LOCATION`.

So `INGEST_LOCATION` is deliberately **not** read from
`MOOSHIK_GEMINI_LOCATION`. Pointing extraction at the embedder's region breaks
it; pointing the embedder at `global` is a separate question this ingester does
not answer.

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

### One binary

The image ships **only `mooshik`**. The serve child used to be a separately
installed `lambo` pinned by SHA — a second copy of the same code, free to
drift from the library Mooshik links, and it did: the image once sat on a rev
whose write params predated `event_time`, and because they are
`deny_unknown_fields` it would have *rejected* every historical write rather
than ignoring the field.

`mooshik serve` publishes lambo's whole MCP surface from the library this repo
already compiles, so that skew is now impossible rather than merely tested
for. It also stamps the **Solo** promotion policy in Rust: a raw `lambo`
defaults to Swarm, which promotes only when independent agents converge, so a
single-writer bootstrap under Swarm fills the graph and promotes nothing —
exactly what M9 measured.

`mooshik serve` takes its session from configuration (`MOOSHIK_SESSION`), not
a flag, and opens an *existing* home — so the entrypoint runs `mooshik init`
first, after the proxy is up. That init creates a vault it never uses: no
secret is stored there (the DSN arrives by environment) and the serve child
opens a vault only when configuration *references* one. The throwaway
passphrase is unset before the child is spawned, and the writer's env
allowlist excludes it regardless.

A raw `lambo serve` is still supported through `INGEST_LAMBO_SERVE` — set
`LAMBO_PROMOTION_POLICY=Solo` if you go that way, or it will promote nothing.

### Write path

The ingester speaks MCP over stdio to a **`mooshik serve` subprocess**
(`INGEST_LAMBO_SERVE`). Lambo's J2 makes a refused `serve` proxy into the
session holder when one exists (Mooshik chat holding the lease), and become
the hub otherwise — either way exactly one graph gets written and the
companion stays up while the corpus loads.

Tools called: `lambo_derive`, `lambo_record_action`, `lambo_recall`.

**Every write carries the document's historical `event_time`** — the commit
author date, or a file's mtime. Lambo's solo promotion policy counts
recurrence over event time and never over flush stamps, so without this a
decade of history arrives as one afternoon, no concept ever clears the
Candidate bar, and canonization promotes nothing. That was measured, not
predicted: M9's first run over this graph found an empty canonical pool.
`created_at` stays server-stamped; `event_time` is the only client-supplied
time Lambo accepts, and omitting it means "a live fact, about now".

Note the surface split: Mooshik's in-process companion tools
(`src/tools/schema.rs`) deliberately have **no** `event_time` — a chat
deriving a fact is asserting it about now. Historical evidence enters only
through this ingester.

Client choice: the official **`mcp` package** (`stdio_client` +
`ClientSession`). It matched lambo's wire shapes with zero friction, so no
hand-rolled JSON-RPC was needed. One sharp edge, fixed in `writer.py`: the
package filters the child environment to a whitelist by default, which
silently strips `LAMBO_*`/DSN config — so the writer builds its own
**targeted allowlist** (`LamboMcpWriter._CHILD_ENV_ALLOWLIST`) of exactly the
variables the serve child needs: `PATH`, `HOME`, `TMPDIR`, `LANG`, `TZ`; the
child's identity (`MOOSHIK_HOME`, `MOOSHIK_SESSION`, `MOOSHIK_AGENT`) and its
store/embedder overlay (`MOOSHIK_STORE_KIND`, `MOOSHIK_EMBEDDER`,
`MOOSHIK_EMBED_DIM`, `MOOSHIK_GEMINI_*`); the `LAMBO_*` equivalents for a raw
lambo child; Gemini credentials (`GCP_LAMBO_CREDENTIALS`,
`GOOGLE_APPLICATION_CREDENTIALS`); and the Postgres DSN authorities
(`MOOSHIK_POSTGRES_DSN`, `LAMBO_POSTGRES_DSN`, `DATABASE_URL`). Everything
else in the parent environment — vault passphrases, cloud tokens, whatever
else a shell accumulates — stays out of the subprocess.

`MOOSHIK_SESSION` earns its place: without it the child would serve the
*default* session and quietly write a bootstrap into the wrong graph.

For the **proxy path** the `lambo serve` child must resolve the same store
as the holder, so a refused start proxies into it instead of opening its own
(SQLite) graph. With a feature-built binary
(`cargo build --features store-postgres,embed-gemini`) export before the
run — only allowlisted names reach the child, so export these exact ones:

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

Delivery is **at-least-once, not exactly-once**: the checkpoint for a
document is marked only after its concepts are written, so a crash between
the last `lambo_derive` and the mark re-extracts and re-writes that document
on the next run. Duplicates, never loss — acceptable for a bootstrap loader,
since the graph tolerates re-derives and M9-style curation can merge or
retract them, whereas a lost extraction would need a full corpus re-read.
A corrupt state file degrades the same way: it is treated as missing and the
run starts clean (full re-ingest). To force a clean re-run deliberately,
delete `.ingest/state.json` and retract the previously written concepts on
the lambo side if accumulation matters. See also the `checkpoint` module
docstring and the M9 heads-up in
`dev-diary/adversarial-review/m8-round1.md`.

## ADK vs genai (decision)

The milestone asks for an ADK agent. `ingester/agent.py` is that ADK shape:
an `LlmAgent` ("given a chunk of text, return concepts") whose function tool
routes through the same writer seam. But the batch pipeline drives
**`google-genai` directly**: M8's loop is a deterministic map over chunks
with checkpointing and rate limiting between calls, and ADK's Runner/session
abstractions are built for interactive multi-turn agents whose history they
want to own — we deliberately keep none. Documented here and in
`dev-diary/adversarial-review/m8-implementation.md`.

## Deployment (Cloud Run Job) — deployed 2026-08-26

The image builds from the repo root (`docker build -f ingester/Dockerfile .`)
and builds the `mooshik` binary from this checkout, plus the Cloud SQL Auth
Proxy. Deployed
as a Cloud Run **Job** (batch, not a serving service): each execution starts
the proxy, runs one ingest pass over `/corpus`, and exits.

One-time setup (already applied on project `mooshik`):

```bash
gcloud services enable run.googleapis.com artifactregistry.googleapis.com \
  cloudbuild.googleapis.com secretmanager.googleapis.com sqladmin.googleapis.com \
  --project mooshik
gcloud artifacts repositories create ingest --repository-format=docker \
  --location=us-central1 --project=mooshik
gcloud projects add-iam-policy-binding mooshik \
  --member=serviceAccount:cachy-nryn@mooshik.iam.gserviceaccount.com \
  --role=roles/cloudsql.client
gcloud artifacts repositories add-iam-policy-binding ingest \
  --location=us-central1 --project=mooshik \
  --member=serviceAccount:cachy-nryn@mooshik.iam.gserviceaccount.com \
  --role=roles/artifactregistry.reader
# Vertex inference lives on nryn-personal:
gcloud projects add-iam-policy-binding nryn-personal \
  --member=serviceAccount:cachy-nryn@mooshik.iam.gserviceaccount.com \
  --role=roles/aiplatform.user
# DSN as a Secret Manager secret, rewritten to the proxy form:
printf '%s' "postgresql://lambo:PW@127.0.0.1:5432/lambo?sslmode=disable" \
  | gcloud secrets create ingest-dsn --project mooshik --data-file=-
gcloud secrets add-iam-policy-binding ingest-dsn --project mooshik \
  --member=serviceAccount:cachy-nryn@mooshik.iam.gserviceaccount.com \
  --role=roles/secretmanager.secretAccessor
```

Build, push, deploy, execute:

```bash
docker build -f ingester/Dockerfile -t ingester:m9 .
docker tag ingester:m9 us-central1-docker.pkg.dev/mooshik/ingest/ingester:m9
gcloud auth configure-docker us-central1-docker.pkg.dev
docker push us-central1-docker.pkg.dev/mooshik/ingest/ingester:m9

gcloud run jobs create ingester --project mooshik --region us-central1 \
  --image us-central1-docker.pkg.dev/mooshik/ingest/ingester:m9 \
  --service-account cachy-nryn@mooshik.iam.gserviceaccount.com \
  --set-secrets=MOOSHIK_POSTGRES_DSN=ingest-dsn:latest,LAMBO_POSTGRES_DSN=ingest-dsn:latest \
  --set-env-vars=LAMBO_STORE=postgres,LAMBO_EMBEDDER=gemini,LAMBO_EMBED_DIM=1536,\
MOOSHIK_GEMINI_PROJECT=nryn-personal,MOOSHIK_GEMINI_LOCATION=us-central1,\
INGEST_LOCATION=global,INGEST_MODEL=gemini-3.7-flash,\
LAMBO_GEMINI_PROJECT=nryn-personal,LAMBO_GEMINI_LOCATION=us-central1 \
  --max-retries 0

gcloud run jobs execute ingester --project mooshik --region us-central1 --wait
```

Notes: the job authenticates via **Application Default Credentials** of the
attached service account — both the auth proxy and `google-genai` pick it up,
so no SA json is baked into or mounted into the image. The proxy is required
because Cloud SQL authorized-networks does not admit Cloud Run egress IPs.
Vertex inference is cross-project (SA from `mooshik`, `roles/aiplatform.user`
on `nryn-personal`). Verified 2026-08-26: execution `ingester-pwd4q`
Completed; concepts extracted inside Cloud Run were recalled by a fresh local
`mooshik recall`.
