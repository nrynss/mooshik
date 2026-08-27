"""Make `news_mcp` importable however pytest was invoked.

`pyproject.toml`'s `pythonpath = ["."]` only applies when pytest picks this
package as its rootdir — true for `pytest mcp-servers/news/tests` (what CI
runs), false when this suite is collected alongside another package's from
the repo root. `ingester` and `measurement` do not need this because they sit
at the repo root and are importable as top-level packages; this one lives two
directories down, under a hyphenated path that can never be one.

Without this, running the whole suite at once fails at import with a message
that points at the test file rather than at the path setup.
"""

import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]

if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))
