"""Fakes for the seams — the whole reason this suite runs offline.

Two seams, faked at two depths, mirroring `ingester/tests/test_ingest.py`:

* `FakeClient` stands in for `genai.Client`. `GroundedBackend` only ever calls
  `client.models.generate_content(...)`, so a fake with that one method
  exercises the real prompt building, the real grounding-metadata reading and
  the real Markdown rendering, with no network and no credentials.

* `ScriptedBackend` stands in for `GroundedBackend` itself. The stdio wire test
  spawns the *real* `build_server()` over one of these, so the JSON-RPC shape
  is proved end to end without the SDK being involved at all.

The response objects below are hand-rolled rather than built from
`google.genai.types`, deliberately: they pin the attribute *path* the renderer
walks (`candidates[].grounding_metadata.grounding_chunks[].web.uri`), which is
the thing that would break under an SDK upgrade.
"""

from __future__ import annotations

from typing import Any


class FakeWeb:
    def __init__(self, uri: str, title: str = "", domain: str = ""):
        self.uri = uri
        self.title = title
        self.domain = domain


class FakeChunk:
    def __init__(self, web: FakeWeb | None = None):
        self.web = web


class FakeGroundingMetadata:
    def __init__(self, chunks=(), queries=()):
        self.grounding_chunks = list(chunks)
        self.web_search_queries = list(queries)


class FakeUrlEntry:
    def __init__(self, retrieved_url: str):
        self.retrieved_url = retrieved_url


class FakeUrlContextMetadata:
    def __init__(self, urls=()):
        self.url_metadata = [FakeUrlEntry(u) for u in urls]


class FakeCandidate:
    def __init__(self, grounding_metadata=None, url_context_metadata=None):
        self.grounding_metadata = grounding_metadata
        self.url_context_metadata = url_context_metadata


class FakeResponse:
    """A grounded response. `text=None` models a blocked/partial candidate."""

    def __init__(self, text: str | None, candidates=()):
        self._text = text
        self.candidates = list(candidates)

    @property
    def text(self) -> str | None:
        if self._text is _RAISES:
            raise ValueError("no parts in candidate")
        return self._text


_RAISES = object()


def raising_response(candidates=()) -> FakeResponse:
    """A response whose `.text` property raises, as the real one can."""
    return FakeResponse(_RAISES, candidates)  # type: ignore[arg-type]


def grounded(text: str, sources=(), queries=()) -> FakeResponse:
    """The common case: prose plus web-grounded sources."""
    chunks = [FakeChunk(FakeWeb(*source)) for source in sources]
    return FakeResponse(
        text, [FakeCandidate(FakeGroundingMetadata(chunks, queries))]
    )


def url_grounded(text: str, urls=()) -> FakeResponse:
    """The `url_context` shape, as `fetch_article` produces."""
    return FakeResponse(
        text, [FakeCandidate(url_context_metadata=FakeUrlContextMetadata(urls))]
    )


class FakeModels:
    def __init__(self, client: "FakeClient"):
        self._client = client

    def generate_content(self, model: str, contents: str, config: Any = None):
        client = self._client
        client.calls.append({"model": model, "contents": contents, "config": config})
        item = client.responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return item


class FakeClient:
    """`genai.Client`, faked at its one used method. Records every call."""

    def __init__(self, responses):
        self.responses = list(responses)
        self.calls: list[dict[str, Any]] = []
        self.models = FakeModels(self)


class ScriptedBackend:
    """`GroundedBackend`, faked for the stdio wire test.

    `secrets` exists because `build_server` reads it off the backend to scrub
    tool output; the wire test uses it to prove that path is wired.
    """

    def __init__(self, answer: str = "canned answer", secrets=(), boom=None):
        self.answer = answer
        self.secrets = tuple(secrets)
        self.boom = boom
        self.calls: list[tuple[str, tuple]] = []

    def search(self, query: str, recency_days: int = 7) -> str:
        self.calls.append(("search", (query, recency_days)))
        if self.boom is not None:
            raise self.boom
        return f"{self.answer}\n\nquery={query} recency_days={recency_days}"

    def fetch(self, url: str, focus: str = "") -> str:
        self.calls.append(("fetch", (url, focus)))
        if self.boom is not None:
            raise self.boom
        return f"{self.answer}\n\nurl={url} focus={focus}"
