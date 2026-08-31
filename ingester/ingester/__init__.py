"""Mooshik bootstrap ingester (M8).

Walks a corpus root, extracts memory concepts with Gemini Flash on Vertex,
and writes them into Mooshik's Cloud SQL graph through `lambo serve` over
MCP. See README.md for usage and decisions.
"""

__version__ = "0.2.0"
