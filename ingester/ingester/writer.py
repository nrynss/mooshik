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

import json
import logging
import shlex
import os
import shlex
from typing import Any
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

log = logging.getLogger(__name__)


class LamboMcpWriter:
    """Async context manager wrapping one `lambo serve` subprocess."""

    def __init__(self, command: str):
        parts = shlex.split(command)
        if not parts:
            raise ValueError("INGEST_LAMBO_SERVE is empty")
        self._params = self._build_params(parts)
        self._transport: Any = None
        self.session: ClientSession | None = None


    @staticmethod
    def _build_params(parts: list[str]) -> StdioServerParameters:
        # The official mcp client filters the child environment down to a
        # safe whitelist by default, which silently strips LAMBO_* / DSN
        # config. The writer contract is a plain subprocess: inherit all.
        return StdioServerParameters(
            command=parts[0], args=parts[1:], env=dict(os.environ)
        )

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
    ) -> Any:
        args: dict[str, Any] = {"agent_id": agent_id, "concepts": concepts}
        if parent_of:
            args["parent_of"] = parent_of
        return await self._call("lambo_derive", args)

    async def record_action(
        self,
        agent_id: str,
        action: str,
        produces: list[str] | None = None,
        modifies: list[str] | None = None,
        depends_on: list[str] | None = None,
    ) -> Any:
        args: dict[str, Any] = {"agent_id": agent_id, "action": action}
        for field, value in (
            ("produces", produces),
            ("modifies", modifies),
            ("depends_on", depends_on),
        ):
            if value:
                args[field] = value
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
