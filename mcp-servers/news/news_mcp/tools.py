"""The MCP tool surface: two tools, described for a small local model.

**Two tools, not six.** Mooshik keeps the companion's whole tool surface to
roughly eight so a small local model routes reliably (`dev-diary/PLAN.md`, M10:
"the companion still sees roughly eight, not forty"). These two are the pair
the spec cut from Rust and expects back through configuration — `search_web`
and `fetch_page` — named for what a user actually asks for.

**Failure is data.** Every tool body runs inside one guard that turns an
upstream error, a timeout, or an empty answer into an ordinary text result the
model can read and act on. Mooshik applies its own per-call bound and contains
a crash itself, but a server that dies rather than answering costs a respawn
and shows the model an opaque internal error, so the containment lives here
too. Nothing below can raise onto the stdio wire.

**stdout is the wire.** Under stdio transport, stdout carries JSON-RPC frames
and one stray `print()` corrupts framing. All logging goes to stderr, which
Mooshik inherits (`src/mcp_host/mod.rs` sets `Stdio::inherit()` for the
child's stderr), so these lines land in the operator's terminal.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Annotated, Any, Callable

from mcp.server.mcpserver import MCPServer
from pydantic import Field

from .errors import NewsToolError, redact

log = logging.getLogger(__name__)

SERVER_NAME = "mooshik-news"

SERVER_INSTRUCTIONS = (
    "Live web lookup for Mooshik. Every answer is grounded in Google Search "
    "results and cites its sources; nothing here is answered from the model's "
    "own memory."
)

SEARCH_DESCRIPTION = (
    "Search the live web and return a short, sourced answer in Markdown. "
    "Use this for news, current events, and any fact that may have changed "
    "or happened since training — prices, releases, outages, weather, who "
    "holds an office now. Ends with a Sources list of links."
)

FETCH_DESCRIPTION = (
    "Read one web page by URL and summarise what it says, in Markdown, citing "
    "the page. Use this when the user gives a link, or to read a source that "
    "search_news returned. Not a search tool: it needs a full http(s) URL."
)

TIMEOUT_TEXT = (
    "Timed out waiting for the web lookup ({secs:.0f}s). The search did not "
    "come back — say so rather than answering from memory, and offer to retry "
    "or narrow the query."
)

UPSTREAM_TEXT = (
    "The web lookup failed upstream ({kind}). No result is available — say so "
    "rather than answering from memory. Detail is on the server's stderr."
)


async def guarded(
    label: str,
    call: Callable[[], str],
    *,
    timeout_secs: float,
    secrets: tuple[str, ...] = (),
) -> str:
    """Run one blocking backend call as a contained, bounded tool result.

    The SDK call is synchronous, so it runs on a worker thread and the wall
    clock is enforced here. A timed-out thread is not killable — it is
    abandoned and unwinds when the SDK's own HTTP timeout fires — but the tool
    answers on time regardless, which is what the caller needs.
    """
    try:
        return await asyncio.wait_for(asyncio.to_thread(call), timeout_secs)
    except NewsToolError as error:
        # Expected and explainable: bad argument, empty query, refused scheme.
        return redact(str(error), secrets)
    except asyncio.TimeoutError:
        log.warning("%s: timed out after %.1fs", label, timeout_secs)
        return TIMEOUT_TEXT.format(secs=timeout_secs)
    except Exception as error:  # noqa: BLE001 - containment is the point
        # The message may quote request state, so only the exception's type
        # reaches the model; the full traceback goes to stderr for the operator.
        log.exception("%s: upstream call failed", label)
        return UPSTREAM_TEXT.format(kind=type(error).__name__)


def build_server(backend: Any, *, timeout_secs: float = 45.0) -> MCPServer:
    """Wire a backend into an MCP server. The backend is the injected seam."""
    secrets = tuple(getattr(backend, "secrets", ()) or ())
    server = MCPServer(
        name=SERVER_NAME,
        title="Mooshik news and web lookup",
        instructions=SERVER_INSTRUCTIONS,
        version="0.1.0",
        log_level="WARNING",
    )

    @server.tool(name="search_news", description=SEARCH_DESCRIPTION)
    async def search_news(
        query: Annotated[
            str,
            Field(description="What to look up, in plain language."),
        ],
        recency_days: Annotated[
            int,
            Field(
                description="Prefer sources from the last N days. Default 7.",
                ge=1,
                le=365,
            ),
        ] = 7,
    ) -> str:
        return await guarded(
            "search_news",
            lambda: backend.search(query, recency_days),
            timeout_secs=timeout_secs,
            secrets=secrets,
        )

    @server.tool(name="fetch_article", description=FETCH_DESCRIPTION)
    async def fetch_article(
        url: Annotated[
            str,
            Field(description="Full http:// or https:// URL of the page to read."),
        ],
        focus: Annotated[
            str,
            Field(description="Optional: what to pay attention to on the page."),
        ] = "",
    ) -> str:
        return await guarded(
            "fetch_article",
            lambda: backend.fetch(url, focus),
            timeout_secs=timeout_secs,
            secrets=secrets,
        )

    return server
