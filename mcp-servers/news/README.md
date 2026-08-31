# Mooshik news MCP server

A **stdio MCP server** that gives Mooshik live web and news lookup, grounded
in **Google Search** through the `google-genai` SDK. Two tools, both returning
clean Markdown with the sources cited.

This is the capability M4 deliberately cut. `search_web` and `fetch_page` were
dropped as hand-written Rust tools because "they come back through M10 as
configured servers instead, which is the whole argument for M10"
(`dev-diary/PLAN.md`). M10 shipped the host; this is the server it was for.
Nothing in the Rust binary changes to gain it — one `config.toml` block and
one permission grant.

## What it exposes

| Tool | Arguments | Returns |
| --- | --- | --- |
| `search_news` | `query` (required), `recency_days` (1–365, default 7) | A short grounded answer in Markdown, the queries that were run, and a `## Sources` list of links |
| `fetch_article` | `url` (required, http/https), `focus` (optional) | What that one page says, in Markdown, with the page cited |

Two, not six, on purpose. Mooshik keeps the companion's whole surface to
roughly eight tools so a small local model routes reliably; a search server
that contributed a dozen would break the local-companion premise rather than
extend it. The descriptions are written for that reader — they say *when to
reach for the tool*, not how it is built.

Provenance is the product. A result flows into the companion's context and can
be written into the user's long-term memory, so an answer with no link is a
claim nobody can check next month. When grounding returns no sources at all,
the result says so in as many words. When an answer is clamped to the
character budget, the body is trimmed and the sources are kept.

## Configuring Mooshik to spawn it

In `~/.mooshik/config.toml`:

```toml
[mcp_servers.news]
command = "python3"
args = ["/absolute/path/to/mcp-servers/news/server.py"]
expose = ["search_news", "fetch_article"]   # empty list = never spawned

[mcp_servers.news.env]
# The KEY is the environment variable the server reads.
# The VALUE is a vault secret NAME — never a literal token.
MOOSHIK_GEMINI_API_KEY = "gemini-api-key"
```

`expose` is an allowlist and it is fail-closed: a server that exposes nothing
is never spawned, and a tool absent from the list is refused even if the
server offers it. Spawning is lazy — the first `specs()` or `execute()` call
starts the child, so a session that never searches pays nothing.

`env` values are **vault secret names**, not values. Mooshik resolves each one
through the encrypted vault at spawn time and injects the result into the
child's environment, so a live credential never sits in a readable config
file. Put the value there with `mooshik secret set gemini-api-key`. There is
no literal-value escape hatch: a name with no matching secret fails *this*
server closed — no tools contributed, other servers unaffected.

**Only secrets need to be listed here.** The child inherits Mooshik's own
environment, so non-secret settings — `MOOSHIK_GEMINI_PROJECT`,
`NEWS_LOCATION`, `NEWS_MODEL`, the timeouts — can simply be exported
wherever Mooshik is started. A Vertex setup on a machine with application
default credentials often needs no `[mcp_servers.news.env]` block at all.

### The permission grant

Mooshik's default grant set allows only the three memory tools and prompts for
`run_scratch_script`; **everything else is denied**. Configured MCP tools
arrive as `mcp.<server>.<tool>` and are gated like every other tool, so
without a grant the companion never sees these two:

```toml
[permissions]
"mcp.news.*" = "allow"          # or "prompt" to confirm each lookup
```

Grant `"mcp.news.*"`, not `web` — `web` is not a known family and parses as a
deny. Per-tool entries win over prefix rules, so `"mcp.news.fetch_article" =
"prompt"` alongside the wildcard is a valid way to auto-allow searching while
confirming page fetches.

## Environment

All configuration is environment-only. Nothing is read from a config file and
no secret is accepted as a command-line argument, where it would land in `ps`
output and shell history — passing any argument at all is an error. A missing
variable exits `2` with a message on stderr naming the variable, and never
prints a value.

| Variable | Default | Meaning |
| --- | --- | --- |
| `MOOSHIK_GEMINI_API_KEY` | — | Gemini Developer API key. When set, runs in API-key mode and needs no project |
| `MOOSHIK_GEMINI_PROJECT` | — | Vertex AI project id. **Required** unless an API key is set |
| `NEWS_LOCATION` | `global` | Vertex region. Its own variable, never `MOOSHIK_GEMINI_LOCATION` — that is the embedder's region, and no Gemini 3.x model is served outside `global` |
| `MOOSHIK_GEMINI_CREDENTIALS` | *(ADC)* | Path to a service-account JSON file |
| `NEWS_MODEL` | `gemini-3.7-flash` | Grounding model |
| `NEWS_TIMEOUT_SECS` | `45` | Per-call wall clock, deliberately inside Mooshik's own 60 s bound |
| `NEWS_MAX_CHARS` | `6000` | Character budget for the answer body, before sources |
| `NEWS_LOG_LEVEL` | `INFO` | Level for this package's stderr logging |

The credential names are the same `MOOSHIK_GEMINI_*` names the bootstrap
ingester reads, so one vault secret serves both.

## Design notes

**No ADK Runner.** `google-adk`'s Runner exists to own multi-turn session
state. An MCP tool call is a single request/response with no turn after it, so
a Runner here would bring a session service that wants to own history this
server deliberately does not keep. The ingester documents the same reasoning
for the same reason (`ingester/ingester/agent.py`). `google-adk` is therefore
not a dependency.

**stdout is the wire.** Under stdio transport stdout carries JSON-RPC frames,
and one stray `print()` corrupts framing. All logging goes to stderr, which
Mooshik inherits for the child, so operator lines land in the terminal. The
root logger is held at `WARNING` so SDK per-request chatter does not drown
them. `mcp` 2.x additionally diverts fd 1 to stderr while serving; the
discipline here does not depend on that.

**Failure is data.** Every tool body runs inside one guard. An upstream error
comes back as a readable sentence naming the exception type, with the
traceback left on stderr; a slow call comes back as a timeout the model can
act on; an empty grounding result comes back as "no result", not as an
exception. Nothing can raise onto the wire. Mooshik applies its own per-call
bound and contains a panicking tool itself, but a server that dies instead of
answering costs a respawn and shows the model an opaque internal error.

**Egress is scrubbed.** Known secret values are replaced with `[redacted]` in
anything this process returns, because an upstream error that quotes a
credential back would be persisted into memory, not merely displayed. Mooshik
redacts tool egress too; this is the same guard one hop earlier, where the
values are actually known.

**Automatic function calling is off.** `google_search` and `url_context` run
on the model server. Leaving AFC enabled would set up a local call loop this
server has no functions for — and an enabled loop is a path by which a
grounded page could ask for a local call.

## Tests

The suite is **offline**: no network, no Google credentials, no ADC. Both
seams are faked, at two depths.

* `FakeClient` stands in for `genai.Client`. `GroundedBackend` only ever calls
  `client.models.generate_content(...)`, so a fake with that one method
  exercises the real prompt building, the real grounding-metadata reading and
  the real Markdown rendering. The response objects are hand-rolled rather
  than built from `google.genai.types`, deliberately: they pin the attribute
  path the renderer walks, which is what would break under an SDK upgrade.
* `ScriptedBackend` stands in for the whole backend. `tests/wire_server.py`
  spawns the **real** `build_server()` over one, and the suite drives it with
  the `mcp` client over a real stdio transport: `initialize`, `tools/list`
  with schema assertions, `tools/call`, an upstream failure that must arrive
  as a result rather than a dead child, a hung backend that must still answer
  within its bound, and a tool body that writes to stdout mid-call and must
  not corrupt framing.

Two further tests run the real `server.py` as a subprocess to prove it fails
closed: no credentials exits `2` with an empty stdout and both variable names
on stderr, and any command-line argument is refused.

```bash
cd mcp-servers/news
python3 -m venv .venv && . .venv/bin/activate
pip install -e ".[dev]"        # or: pip install mcp==2.1.1 google-genai==2.20.0 pytest==9.1.1
pytest -q
```

From the repo root, `pytest mcp-servers/news/tests -q` works unchanged — which
is what CI's `news-mcp` job runs.

## Running it by hand

```bash
export MOOSHIK_GEMINI_PROJECT=your-project
export MOOSHIK_GEMINI_CREDENTIALS=/path/to/service-account.json
python3 mcp-servers/news/server.py      # speaks MCP on stdin/stdout
```

There is nothing to look at — it is waiting for JSON-RPC frames. Drive it with
any MCP client, or let Mooshik spawn it. `python3 -m news_mcp` from this
directory is the same program.
