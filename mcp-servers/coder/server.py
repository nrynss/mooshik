#!/usr/bin/env python3
"""Path-addressable entry point, for the `args = [".../server.py"]` config.

Mooshik spawns MCP servers by command and argument list, with no shell and no
working directory of its own, so the documented invocation is an absolute path
to this file. It adds its own directory to `sys.path` and defers to the
package, which keeps `python3 /abs/path/server.py` and `python3 -m coder_mcp`
the same program.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from coder_mcp.__main__ import main  # noqa: E402 - after the sys.path fix-up

if __name__ == "__main__":
    raise SystemExit(main())
