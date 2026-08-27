"""MCP writer bridge: the ingester speaks to `lambo serve` over stdio.

Lambo's J2 makes a *refused* `lambo serve` proxy into the session holder
(Mooshik chat, M2b) when one exists, and become the hub otherwise — so the
ingester never needs to know which side of the lease it landed on. One graph
gets written either way.

Client choice (documented in README): the official `mcp` package
(`mcp.client.stdio.stdio_client` + `ClientSession`). It speaks the same wire
shapes as lambo's server, so no hand-rolled JSON-RPC is needed.

The command is `INGEST_LAMBO_SERVE` (default `lambo serve`) and is parsed
with `shlex` so a full path plus flags works.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shlex
import time
from typing import Any
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

log = logging.getLogger(__name__)

async def drain(
    writer: "LamboMcpWriter",
    agent_id: str,
    timeout: float = 60.0,
    poll: float = 0.5,
) -> bool:
    """Poll `lambo_stats` until the write-behind log is empty.

    The pipeline's last derive acks before the embedder runs (J3), so an
    abrupt teardown after the final call can still discard the un-embedded
    tail. Draining on log_depth == 0 before tearing the child down makes
    every acknowledged write durable — which matters most where the child
    dies with the process (Cloud Run Jobs).
    """
    deadline = time.monotonic() + timeout
    while True:
        try:
            stats = await writer._call("lambo_stats", {"agent_id": agent_id})
        except Exception:
            stats = None
        if isinstance(stats, dict) and stats.get("log_depth") == 0:
            return True
        if time.monotonic() >= deadline:
            return False
        await asyncio.sleep(poll)


class LamboMcpWriter:
    """Async context manager wrapping one `lambo serve` subprocess."""

    def __init__(self, command: str):
        parts = shlex.split(command)
        if not parts:
            raise ValueError("INGEST_LAMBO_SERVE is empty")
        self._params = self._build_params(parts)
        self._transport: Any = None
        self.session: ClientSession | None = None


    #: Environment handed to the `lambo serve` child: a targeted allowlist,
    #: not wholesale inheritance. The mcp package's own default whitelist
    #: strips even LAMBO_*/DSN config, so the answer is to enumerate exactly
    #: what the child needs (documented in ingester/README.md "Write path")
    #: rather than pass every parent variable — including vault passphrases
    #: and cloud tokens — into a subprocess whose log/config surface we do
    #: not control.
    _CHILD_ENV_ALLOWLIST: tuple[str, ...] = (
        # bare process essentials
        "PATH",
        "HOME",
        "TMPDIR",
        "LANG",
        "TZ",
        # store / embedder resolution for the proxy-or-hub serve child
        "LAMBO_STORE",
        "LAMBO_EMBEDDER",
        "LAMBO_EMBED_DIM",
        # Gemini embedder credentials (lambo adapter reads these names)
        "LAMBO_GEMINI_PROJECT",
        "LAMBO_GEMINI_LOCATION",
        "LAMBO_GEMINI_CREDENTIALS",
        "GCP_LAMBO_CREDENTIALS",
        "GOOGLE_APPLICATION_CREDENTIALS",
        # Postgres DSN authorities (Mooshik overlay + lambo native names)
        "MOOSHIK_POSTGRES_DSN",
        "LAMBO_POSTGRES_DSN",
        "DATABASE_URL",
    )

    @classmethod
    def _build_params(cls, parts: list[str]) -> StdioServerParameters:
        env = {
            name: value
            for name, value in os.environ.items()
            if name in cls._CHILD_ENV_ALLOWLIST
        }
        return StdioServerParameters(command=parts[0], args=parts[1:], env=env)

    async def __aenter__(self) -> "LamboMcpWriter":
        self._transport = stdio_client(self._params)
        read, write = await self._transport.__aenter__()
        self.session = ClientSession(read, write)
        await self.session.__aenter__()
        await self.session.initialize()
        return self

    async def __aexit__(self, *exc_info: object) -> None:
        if self.session is not None:
            await self.session.__aexit__(*exc_info)
            self.session = None
        if self._transport is not None:
            await self._transport.__aexit__(*exc_info)
            self._transport = None

    async def _call(self, tool: str, arguments: dict[str, Any]) -> Any:
        if self.session is None:
            raise RuntimeError("writer not started: use 'async with'")
        result = await self.session.call_tool(tool, arguments)
        if getattr(result, "isError", False):
            text = result.content[0].text if result.content else ""
            raise RuntimeError(f"{tool} failed: {text}")
        if not result.content:
            return None
        raw = result.content[0].text
        try:
            return json.loads(raw)
        except ValueError:
            return raw

    async def derive(
        self,
        agent_id: str,
        concepts: list[dict[str, str]],
        parent_of: list[dict[str, str]] | None = None,
        event_time: str | None = None,
    ) -> Any:
        args: dict[str, Any] = {"agent_id": agent_id, "concepts": concepts}
        if parent_of:
            args["parent_of"] = parent_of
        if event_time:
            args["event_time"] = event_time
        return await self._call("lambo_derive", args)

    async def record_action(
        self,
        agent_id: str,
        action: str,
        produces: list[str] | None = None,
        modifies: list[str] | None = None,
        depends_on: list[str] | None = None,
        event_time: str | None = None,
    ) -> Any:
        args: dict[str, Any] = {"agent_id": agent_id, "action": action}
        for field, value in (
            ("produces", produces),
            ("modifies", modifies),
            ("depends_on", depends_on),
        ):
            if value:
                args[field] = value
        if event_time:
            args["event_time"] = event_time
        return await self._call("lambo_record_action", args)

    async def recall(
        self,
        agent_id: str,
        query: str,
        top_k: int | None = None,
    ) -> Any:
        args: dict[str, Any] = {"agent_id": agent_id, "query": query}
        if top_k is not None:
            args["top_k"] = top_k
        return await self._call("lambo_recall", args)
