"""The MCP tool surface: two tools, described for a small local model.

**Two tools, not six.** Mooshik keeps the companion's whole tool surface to
roughly eight so a small local model routes reliably (``dev-diary/PLAN.md``,
M10). These two are the pair the spec names — ``delegate`` fires a coding
agent against a repository and returns immediately; ``check`` reports
whether it is still running.

**Failure is data.** Every tool body runs inside one guard that turns an
upstream error, a timeout, or an empty answer into an ordinary text result
the model can read and act on. Mooshik applies its own per-call bound and
contains a crash itself, but a server that dies rather than answering costs
a respawn and shows the model an opaque internal error, so the containment
lives here too. Nothing below can raise onto the stdio wire.

**stdout is the wire.** Under stdio transport, stdout carries JSON-RPC frames
and one stray ``print()`` corrupts framing. All logging goes to stderr, which
Mooshik inherits (``src/mcp_host/mod.rs`` sets ``Stdio::inherit()`` for the
child's stderr), so these lines land in the operator's terminal.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Annotated, Any, Callable

from mcp.server.mcpserver import MCPServer
from pydantic import Field

from .errors import CoderToolError, redact

log = logging.getLogger(__name__)

SERVER_NAME = "mooshik-coder"

SERVER_INSTRUCTIONS = (
    "Coding contractor delegation for Mooshik. Spawns a coding agent "
    "(Claude Code, Gemini CLI, Cursor Agent, or Antigravity / agy) against a repository and "
    "reports its status. The agent works under standing constraints drawn "
    "from the workspace memory graph."
)

DELEGATE_DESCRIPTION = (
    "Delegate a code change to the configured coding agent. Spawns the "
    "agent against the given repository and returns immediately with a "
    "handle you can poll with check. The agent reads AGENTS.md in the repo "
    "root for standing constraints from the workspace memory graph."
)

CHECK_DESCRIPTION = (
    "Check whether a previously delegated coding agent is still running. "
    "Returns the handle's status: running, exited (with exit code), or "
    "unknown if the handle was not found."
)

TIMEOUT_TEXT = (
    "Timed out waiting for the tool ({secs:.0f}s). The call did not come "
    "back — say so rather than guessing, and offer to retry."
)

UPSTREAM_TEXT = (
    "The tool call failed ({kind}). No result is available — say so rather "
    "than guessing. Detail is on the server's stderr."
)


async def guarded(
    label: str,
    call: Callable[[], str],
    *,
    timeout_secs: float,
    secrets: tuple[str, ...] = (),
) -> str:
    """Run one blocking backend call as a contained, bounded tool result.

    The backend call is synchronous, so it runs on a worker thread and the
    wall clock is enforced here. A timed-out thread is not killable — it is
    abandoned and unwinds when its own work finishes — but the tool answers
    on time regardless, which is what the caller needs.
    """
    try:
        return await asyncio.wait_for(asyncio.to_thread(call), timeout_secs)
    except CoderToolError as error:
        # Expected and explainable: bad argument, missing repo.
        return redact(str(error), secrets)
    except asyncio.TimeoutError:
        log.warning("%s: timed out after %.1fs", label, timeout_secs)
        return TIMEOUT_TEXT.format(secs=timeout_secs)
    except Exception as error:  # noqa: BLE001 - containment is the point
        # The message may quote request state, so only the exception's type
        # reaches the model; the full traceback goes to stderr for the operator.
        log.exception("%s: upstream call failed", label)
        return UPSTREAM_TEXT.format(kind=type(error).__name__)


def build_server(backend: Any, *, timeout_secs: float = 10.0) -> MCPServer:
    """Wire a backend into an MCP server. The backend is the injected seam."""
    secrets = tuple(getattr(backend, "secrets", ()) or ())
    server = MCPServer(
        name=SERVER_NAME,
        title="Mooshik coding contractor",
        instructions=SERVER_INSTRUCTIONS,
        version="0.2.0",
        log_level="WARNING",
    )

    @server.tool(name="delegate", description=DELEGATE_DESCRIPTION)
    async def delegate(
        task: Annotated[
            str,
            Field(
                description=(
                    "What code change to make, in plain language. Be specific "
                    "about files, functions, and the desired outcome."
                )
            ),
        ],
        repo: Annotated[
            str,
            Field(
                description=(
                    "Absolute path to the repository the agent should work in."
                )
            ),
        ],
    ) -> str:
        return await guarded(
            "delegate",
            lambda: backend.delegate(task, repo),
            timeout_secs=timeout_secs,
            secrets=secrets,
        )

    @server.tool(name="check", description=CHECK_DESCRIPTION)
    async def check(
        handle: Annotated[
            str,
            Field(
                description=(
                    "The handle returned by a previous delegate call."
                )
            ),
        ],
    ) -> str:
        return await guarded(
            "check",
            lambda: backend.check(handle),
            timeout_secs=timeout_secs,
            secrets=secrets,
        )

    return server
