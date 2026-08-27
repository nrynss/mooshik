"""The child process for the stdio wire test.

This is the *real* `build_server()` — the same function `news_mcp.__main__`
calls — over a scripted backend, so an `initialize` + `tools/list` +
`tools/call` round trip proves the actual wire shape without a Google client
existing anywhere. `WIRE_MODE` picks which failure the backend performs.

Run directly by the test through `mcp.client.stdio`; it is not a test module
itself (hence the name, which pytest will not collect).
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))  # news_mcp
sys.path.insert(0, str(HERE))  # fakes

from fakes import ScriptedBackend  # noqa: E402 - after the sys.path fix-up
from news_mcp.errors import NewsToolError  # noqa: E402
from news_mcp.tools import build_server  # noqa: E402


class NoisyBackend(ScriptedBackend):
    """Writes to stdout mid-call — the thing that corrupts JSON-RPC framing."""

    def search(self, query: str, recency_days: int = 7) -> str:
        print("STRAY STDOUT FROM A TOOL BODY")
        sys.stdout.flush()
        return super().search(query, recency_days)


def make_backend() -> ScriptedBackend:
    mode = os.environ.get("WIRE_MODE", "ok")
    if mode == "boom":
        return ScriptedBackend(boom=RuntimeError("upstream 503: quota gremlins"))
    if mode == "refused":
        return ScriptedBackend(boom=NewsToolError("Unsupported URL scheme in 'file'."))
    if mode == "hang":
        return _HangingBackend()
    if mode == "noisy":
        return NoisyBackend()
    return ScriptedBackend()


class _HangingBackend(ScriptedBackend):
    def search(self, query: str, recency_days: int = 7) -> str:
        import time

        time.sleep(10)
        return "never reached"


if __name__ == "__main__":
    timeout = float(os.environ.get("WIRE_TIMEOUT_SECS", "10"))
    build_server(make_backend(), timeout_secs=timeout).run("stdio")
