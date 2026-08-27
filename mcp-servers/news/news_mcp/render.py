"""Grounded responses in, clean Markdown with sources out.

Two reasons this is its own module rather than string-building inside the
backend. First, provenance is the point: a result flows into the companion's
context and may be written into the user's long-term memory, so a claim that
arrives without a link is a claim nobody can check later. Second, the shape of
a grounding response is the one part of the SDK surface worth pinning with
tests, and a pure function over a response object is trivially testable with a
fake.

Everything here reads defensively with `getattr`. Grounding metadata is
optional at every level, and a partial response should degrade to "text with
no sources", never to a traceback on the stdio wire.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable

#: Appended when the clamp bites, so the model can tell "short answer" from
#: "long answer, truncated" and offer to narrow the query.
TRUNCATION_NOTE = "\n\n_[truncated]_"

#: What a tool returns when the model answered with nothing at all.
EMPTY_RESULT = (
    "No result. The search returned nothing usable for this query — "
    "try different or more specific wording."
)


@dataclass(frozen=True)
class Source:
    """One cited web source."""

    uri: str
    title: str = ""
    domain: str = ""

    def bullet(self) -> str:
        label = self.title or self.domain or self.uri
        suffix = f" ({self.domain})" if self.domain and self.domain not in label else ""
        return f"- [{label}]({self.uri}){suffix}"


def _candidates(response: Any) -> Iterable[Any]:
    return getattr(response, "candidates", None) or ()


def answer_text(response: Any) -> str:
    """The model's prose, or an empty string.

    `response.text` is a property on the real SDK object and raises on some
    partial responses (no parts, blocked candidate), so it is read inside a
    guard rather than trusted.
    """
    try:
        text = response.text
    except Exception:  # noqa: BLE001 - a partial response must not crash the wire
        text = None
    return (text or "").strip()


def search_queries(response: Any) -> list[str]:
    """The queries grounding actually ran, for provenance."""
    seen: list[str] = []
    for candidate in _candidates(response):
        metadata = getattr(candidate, "grounding_metadata", None)
        for query in getattr(metadata, "web_search_queries", None) or ():
            if isinstance(query, str) and query.strip() and query not in seen:
                seen.append(query.strip())
    return seen


def collect_sources(response: Any) -> list[Source]:
    """Every web source cited by grounding, de-duplicated, order preserved.

    Covers both grounding shapes the two tools produce: `google_search` fills
    `grounding_metadata.grounding_chunks[].web`, and `url_context` fills
    `url_context_metadata.url_metadata[].retrieved_url`.
    """
    sources: list[Source] = []
    seen: set[str] = set()

    def add(uri: Any, title: Any = "", domain: Any = "") -> None:
        if not isinstance(uri, str) or not uri.strip() or uri in seen:
            return
        seen.add(uri)
        sources.append(
            Source(
                uri=uri.strip(),
                title=(title or "").strip() if isinstance(title, str) else "",
                domain=(domain or "").strip() if isinstance(domain, str) else "",
            )
        )

    for candidate in _candidates(response):
        metadata = getattr(candidate, "grounding_metadata", None)
        for chunk in getattr(metadata, "grounding_chunks", None) or ():
            web = getattr(chunk, "web", None)
            if web is not None:
                add(
                    getattr(web, "uri", None),
                    getattr(web, "title", "") or "",
                    getattr(web, "domain", "") or "",
                )
        url_metadata = getattr(candidate, "url_context_metadata", None)
        for entry in getattr(url_metadata, "url_metadata", None) or ():
            add(getattr(entry, "retrieved_url", None))

    return sources


def clamp(text: str, max_chars: int) -> str:
    """Trim to a character budget on a whitespace boundary where possible."""
    if max_chars <= 0 or len(text) <= max_chars:
        return text
    cut = text[:max_chars]
    boundary = cut.rfind("\n")
    if boundary < max_chars // 2:
        boundary = cut.rfind(" ")
    if boundary > max_chars // 2:
        cut = cut[:boundary]
    return cut.rstrip() + TRUNCATION_NOTE


def render(response: Any, *, max_chars: int, show_queries: bool = False) -> str:
    """A grounded response as Markdown: body, then a Sources list.

    The body is clamped *before* the sources are appended, so the citations
    survive truncation — a truncated answer with links beats a full answer
    nobody can verify.
    """
    body = answer_text(response)
    sources = collect_sources(response)
    if not body and not sources:
        return EMPTY_RESULT

    parts = [clamp(body, max_chars) if body else EMPTY_RESULT]

    if show_queries:
        queries = search_queries(response)
        if queries:
            joined = ", ".join(f"`{query}`" for query in queries)
            parts.append(f"_Searched: {joined}_")

    if sources:
        parts.append("## Sources\n" + "\n".join(source.bullet() for source in sources))
    else:
        parts.append("_No sources were cited for this answer; treat it as unverified._")

    return "\n\n".join(parts)
