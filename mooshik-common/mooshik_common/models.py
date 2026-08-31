"""Which Gemini model, and which Vertex location — in one place.

Every Python component in this repo talks to Vertex, and before this module
existed each carried its own copy of these two constants. That duplication was
not theoretical: on 2026-08-31 the *same pair* of defects was found and fixed
separately in `ingester/` and in `mcp-servers/news/` — a model below the
supported floor, and an inference call pointed at the embedder's region.
"""

from __future__ import annotations

#: The inference model every component defaults to.
DEFAULT_MODEL = "gemini-3.7-flash"

#: The only Vertex location that serves Gemini 3.x. Requesting `gemini-3.5`,
#: `3.6` or `3.7` in a *region* answers
#: `404 NOT_FOUND: Publisher model ... was not found or your project does not
#: have access to it` — verified live 2026-08-31 for all three. It is also
#: where Search grounding is served, so inference has two reasons to sit here.
GLOBAL_LOCATION = "global"

#: The location any inference call should default to.
DEFAULT_LOCATION = GLOBAL_LOCATION

#: The embedder's region variable. **Never read this for an inference call.**
#:
#: It names where `gemini-embedding-001` lives (`us-central1`), and the Cloud
#: Run deploy maps it onward to `LAMBO_GEMINI_LOCATION`. Two models, two
#: regions: pointing inference at the embedder's region breaks inference, and
#: pointing the embedder at `global` is a separate question no component here
#: answers. Each component gives its inference location its own variable —
#: `INGEST_LOCATION`, `NEWS_LOCATION` — precisely so this one cannot leak in.
EMBEDDER_LOCATION_ENV = "MOOSHIK_GEMINI_LOCATION"
