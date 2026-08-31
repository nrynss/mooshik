---
title: Research and the Web
description: Conduct live web research with cited Markdown sources via the news MCP server.
---

Mooshik includes live web research through its `news` MCP server. The server grounds responses in Google Search and returns clean, cited Markdown.

## Why Grounded Research Matters

When an assistant writes facts into your long-term memory graph, provenance is essential. An unsourced assertion cannot be audited next month.

The news server ensures every factual claim cites its origin. Answers include a `## Sources` section listing the underlying URLs.

## Available Research Tools

The news server exposes two tools:

| Tool | Arguments | Purpose |
| :--- | :--- | :--- |
| `search_news` | `query` (required), `recency_days` (default 7) | Performs grounded search and returns a synthesized Markdown answer with cited links. |
| `fetch_article` | `url` (required), `focus` (optional) | Extracts and converts web page content into clean Markdown text. |

## Enabling the News Server

Add the server block to `~/.mooshik/config.toml`:

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
MOOSHIK_GEMINI_API_KEY = "gemini-api-key"
```

Set the permission grant to allow the companion to run research queries:

```toml
[permissions]
"mcp.news.*" = "allow"
```

## Example Usage in the Pane

Ask research questions directly in `mooshik tui`:

```text
> Check latest release notes for Rust 1.97 and summarize compiler changes.
```

The companion invokes `search_news`, receives structured Markdown citations, displays the summary in the pane, and stores the decision context in memory.
