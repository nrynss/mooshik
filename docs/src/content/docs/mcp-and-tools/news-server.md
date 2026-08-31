---
title: News MCP Server
description: Live web search and URL grounding over stdio MCP.
---

The News MCP server provides live web search and article reading tools grounded in Google Search.

## Provided Tools

### 1. `search_news`
Searches the live web and returns concise grounded summaries with source citations.

Parameters:
- `query`: The search topic in plain language.
- `recency_days`: Restricts results to recent days (default 7).

### 2. `fetch_article`
Fetches and summarizes content from a specific web URL.

Parameters:
- `url`: Full HTTP or HTTPS web address.
- `focus`: Optional topic or key point to extract.

## Configuration

Configure the server in `~/.mooshik/config.toml`:

```toml
[mcp.servers.news]
command = "python3"
args = ["/path/to/mooshik/mcp-servers/news/server.py"]
env = { NEWS_LOCATION = "global" }
```

## Running Standalone

You can test the server directly over stdio:

```sh
python3 mcp-servers/news/server.py
```
