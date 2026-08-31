---
title: News and Search Server
description: Grounded web search and article fetching with cited sources via the news MCP server.
---

The `news` server is a stdio MCP server that provides live web research and article extraction grounded in Google Search via the `google-genai` SDK.

## Exposed Tools

| Tool | Arguments | Returns |
| :--- | :--- | :--- |
| `search_news` | `query` (required), `recency_days` (1–365, default 7) | A concise Markdown summary, the search queries executed, and a `## Sources` section with links. |
| `fetch_article` | `url` (required), `focus` (optional) | The article text converted to clean Markdown with the source cited. |

## Provenance and Citation

Factual assertions stored in long-term memory must be verifiable. When `search_news` executes, it includes cited links in the response body. If grounding finds no sources or the content is truncated to fit the character budget, the response explicitly indicates the limitation.

## Server Configuration

Configure the server in `~/.mooshik/config.toml`:

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
MOOSHIK_GEMINI_PROJECT = "gemini-project"
```

Grant execution permissions:

```toml
[permissions]
"mcp.news.*" = "allow"
```

## Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `MOOSHIK_GEMINI_API_KEY` | *(unset)* | Gemini Developer API key (if using API key auth). |
| `MOOSHIK_GEMINI_PROJECT` | *(unset)* | Google Cloud Vertex AI project ID. |
| `NEWS_LOCATION` | `global` | Vertex AI inference region. |
| `MOOSHIK_GEMINI_CREDENTIALS` | *(ADC)* | Path to Google Cloud service account JSON file. |
| `NEWS_MODEL` | `gemini-3.7-flash` | Grounding and synthesis model. |
| `NEWS_TIMEOUT_SECS` | `45` | Internal per-call timeout. |
| `NEWS_MAX_CHARS` | `6000` | Character limit for the returned Markdown body. |

## Design Safeguards

- **No multi-turn runner overhead:** Tool calls are single request-response invocations without unnecessary session state management.
- **Wire isolation:** Standard output carries JSON-RPC protocol frames only. Diagnostic logging routes to standard error.
- **Egress redaction:** Known credentials and tokens in the server environment are replaced with `[redacted]` before error strings cross the wire.
