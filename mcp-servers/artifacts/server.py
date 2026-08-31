#!/usr/bin/env python3
"""Path-addressable entry point, for the `args = [".../server.py"]` config."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from artifacts_mcp.__main__ import main
if __name__ == "__main__":
    raise SystemExit(main())
