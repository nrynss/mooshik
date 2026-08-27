"""Mooshik's news/web-lookup MCP server.

A stdio MCP server exposing two tools — `search_news` and `fetch_article` —
backed by Google Search grounding through the `google-genai` SDK. Mooshik
spawns it as a child process and surfaces its tools to the companion model as
`mcp.news.<tool>` (see `src/mcp_host/mod.rs`).
"""

__all__ = ["__version__"]

__version__ = "0.1.0"
