from __future__ import annotations
import asyncio
import logging
from typing import Annotated, Any, Callable
from mcp.server.mcpserver import MCPServer
from pydantic import Field
from .errors import ArtifactsToolError, redact

log = logging.getLogger(__name__)
SERVER_NAME = "mooshik-artifacts"
SERVER_INSTRUCTIONS = "Extract typed concepts from non-text workspace artifacts (screenshots, audio)."

async def guarded(label: str, call: Callable[[], str], timeout_secs: float, secrets: tuple[str, ...] = ()) -> str:
    try:
        return await asyncio.wait_for(asyncio.to_thread(call), timeout_secs)
    except ArtifactsToolError as error:
        return redact(str(error), secrets)
    except asyncio.TimeoutError:
        log.warning("%s: timed out after %.1fs", label, timeout_secs)
        return f"Timed out waiting for extraction ({timeout_secs:.0f}s)."
    except Exception as error:
        log.exception("%s: upstream call failed", label)
        return f"Extraction failed upstream ({type(error).__name__}). Detail is on the server's stderr."

def build_server(backend: Any, timeout_secs: float = 45.0) -> MCPServer:
    secrets = tuple(getattr(backend, "secrets", ()) or ())
    server = MCPServer(
        name=SERVER_NAME,
        title="Mooshik artifacts extraction",
        instructions=SERVER_INSTRUCTIONS,
        version="0.2.1",
        log_level="WARNING",
    )

    @server.tool(name="extract_concepts", description="Extract memory concepts from a non-text file.")
    async def extract_concepts(
        file_path: Annotated[str, Field(description="Absolute path to the image or audio file.")]
    ) -> str:
        return await guarded(
            "extract_concepts",
            lambda: backend.extract(file_path),
            timeout_secs=timeout_secs,
            secrets=secrets,
        )

    return server
