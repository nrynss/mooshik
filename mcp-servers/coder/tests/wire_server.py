"""Standalone wire harness: a real MCP stdio server backed by ScriptedBackend.

Spawned as a subprocess by the test suite to exercise the full JSON-RPC
round trip — ``initialize``, ``list_tools``, ``call_tool`` — exactly as
Mooshik's host would. The ``WIRE_MODE`` environment variable selects the
backend flavour:

* ``ok``    — canned success answers (default)
* ``boom``  — every call raises a ``RuntimeError``
* ``hang``  — every call blocks for 10 seconds
* ``noisy`` — prints to stdout before answering (framing corruption test)
"""

import os
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

from fakes import ScriptedBackend
from coder_mcp.errors import CoderToolError
from coder_mcp.tools import build_server


class NoisyBackend(ScriptedBackend):
    """A backend that prints to stdout before answering — the corruption test."""

    def delegate(self, task: str, repo: str) -> str:
        print("STRAY STDOUT FROM A TOOL BODY")
        sys.stdout.flush()
        return super().delegate(task, repo)


def make_backend() -> ScriptedBackend:
    mode = os.environ.get("WIRE_MODE", "ok")
    if mode == "boom":
        return ScriptedBackend(boom=RuntimeError("upstream 503"))
    if mode == "refused":
        return ScriptedBackend(boom=CoderToolError("Bad repo path."))
    if mode == "hang":
        class _Hanging(ScriptedBackend):
            def delegate(self, task: str, repo: str) -> str:
                time.sleep(10)
                return "never"
            def check(self, handle: str) -> str:
                time.sleep(10)
                return "never"
        return _Hanging()
    if mode == "noisy":
        return NoisyBackend()
    return ScriptedBackend()


if __name__ == "__main__":
    timeout = float(os.environ.get("WIRE_TIMEOUT_SECS", "10"))
    build_server(make_backend(), timeout_secs=timeout).run("stdio")
