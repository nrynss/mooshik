"""Mooshik's coding contractor MCP server.

A stdio MCP server exposing two tools — ``delegate`` and ``check`` — that
spawn and monitor external coding agents (Claude Code, Gemini CLI / OMP,
Cursor Agent CLI, Antigravity CLI / agy). Mooshik spawns it as a child process and surfaces its
tools to the companion model as ``mcp.coder.<tool>`` (see
``src/mcp_host/mod.rs``).

Unlike the news and artifacts servers, this one calls no Google SDK and
makes no inference request. It spawns a coding agent as a subprocess,
returns immediately, and reports liveness on demand. The *result* arrives
the ambient way: M12d's workspace watcher sees the edits land and derives
them, so the pane fills with what the contractor did while it is still
working. Nothing has to marshal a diff back through a tool call.
"""

__all__ = ["__version__"]

__version__ = "0.2.0"
