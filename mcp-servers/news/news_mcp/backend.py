"""The Google Search grounding seam.

**Why no ADK Runner.** `google-adk`'s Runner owns multi-turn session state —
it exists to carry history across turns and replay it. An MCP tool call is a
single request/response with no turn after it, so a Runner here would add a
session service that wants to own history this server deliberately does not
keep. The bootstrap ingester documents the same reasoning for the same reason
(`ingester/ingester/agent.py`, `ingester/ingester/extraction.py`): drive
`google-genai` directly, and let the caller own the loop. Mooshik *is* the
loop here.

**The seam.** `GroundedBackend` takes an already-built client object and only
ever touches `client.models.generate_content(...)`. Production passes a
`genai.Client`; the offline suite passes a fake, exactly as
`ingester/tests/test_ingest.py` fakes its extractor client. Nothing in this
module opens a socket at import time, so `pytest` runs with no network and no
credentials.
"""

from __future__ import annotations

import datetime as _datetime
import logging
from typing import Any, Sequence

from google.genai import types

from .errors import NewsToolError, redact
from .render import collect_sources, render

__all__ = [
    "GroundedBackend",
    "NewsToolError",
    "clamp_recency",
    "fetch_prompt",
    "make_client",
    "normalise_url",
    "redact",
    "search_prompt",
]

log = logging.getLogger(__name__)

#: Bounds on the recency window a caller may ask for, in days.
MIN_RECENCY_DAYS = 1
MAX_RECENCY_DAYS = 365

SEARCH_INSTRUCTION = (
    "You are a research assistant answering with the live web.\n"
    "Answer the question below in Markdown: a two-to-five sentence summary, "
    "then short bullets for the individual items when there is more than one.\n"
    "Rules: state only what the search results support; attribute claims to "
    "the outlet reporting them; say plainly when the sources disagree or when "
    "you found nothing; no preamble, no offers of further help; never invent "
    "a URL."
)

FETCH_INSTRUCTION = (
    "Read the page at the URL below and report what it says, in Markdown.\n"
    "Rules: summarise only what is actually on the page; quote sparingly and "
    "mark quotes as quotes; if the page cannot be retrieved or is not the "
    "expected content, say so in one line instead of guessing; no preamble."
)


def clamp_recency(days: Any) -> int:
    """Coerce a caller-supplied recency window into the supported range."""
    try:
        value = int(days)
    except (TypeError, ValueError):
        return 7
    return max(MIN_RECENCY_DAYS, min(MAX_RECENCY_DAYS, value))


def normalise_url(url: Any) -> str:
    """Validate a caller-supplied URL, or raise a message worth showing.

    Only http/https: this tool reaches the public web through Gemini, and a
    `file:` or `data:` argument is either a mistake or an attempt to make the
    tool read something local.
    """
    if not isinstance(url, str) or not url.strip():
        raise NewsToolError("No URL given. Pass a full http:// or https:// URL.")
    candidate = url.strip()
    if not candidate.lower().startswith(("http://", "https://")):
        raise NewsToolError(
            f"Unsupported URL scheme in {candidate.split(':', 1)[0]!r}: "
            "fetch_article reads public web pages over http:// or https:// only."
        )
    return candidate


def search_prompt(query: str, recency_days: int, today: str) -> str:
    return (
        f"{SEARCH_INSTRUCTION}\n\n"
        f"Today's date is {today}. Prefer sources published within the last "
        f"{recency_days} day(s); if the best available reporting is older, use "
        f"it and say how old it is.\n\n"
        f"QUESTION:\n{query.strip()}"
    )


def fetch_prompt(url: str, focus: str) -> str:
    focus_line = (
        f"\n\nFocus on this in particular: {focus.strip()}" if focus.strip() else ""
    )
    return f"{FETCH_INSTRUCTION}\n\nURL:\n{url}{focus_line}"


def _today() -> str:
    return _datetime.date.today().isoformat()


class GroundedBackend:
    """Grounded lookups over an injected `google-genai`-shaped client."""

    def __init__(
        self,
        client: Any,
        *,
        model: str = "gemini-2.5-flash",
        max_chars: int = 6_000,
        timeout_secs: float = 45.0,
        secrets: Sequence[str] = (),
        clock=_today,
    ):
        self.client = client
        self.model = model
        self.max_chars = max_chars
        self.timeout_secs = timeout_secs
        self.secrets = tuple(s for s in secrets if s)
        self.clock = clock

    # -- tool bodies ------------------------------------------------------

    def search(self, query: str, recency_days: int = 7) -> str:
        """Grounded web search; Markdown answer with a Sources list."""
        if not isinstance(query, str) or not query.strip():
            raise NewsToolError("No query given. Pass what you want to look up.")
        prompt = search_prompt(query, clamp_recency(recency_days), self.clock())
        tool = types.Tool(google_search=types.GoogleSearch())
        return self._generate(prompt, tool, show_queries=True)

    def fetch(self, url: str, focus: str = "") -> str:
        """Retrieve one URL and summarise it; Markdown with the source cited."""
        target = normalise_url(url)
        prompt = fetch_prompt(target, focus if isinstance(focus, str) else "")
        tool = types.Tool(url_context=types.UrlContext())
        return self._generate(prompt, tool, show_queries=False)

    # -- the one SDK call -------------------------------------------------

    def _generate(self, prompt: str, tool: Any, *, show_queries: bool) -> str:
        config = types.GenerateContentConfig(
            tools=[tool],
            temperature=0.0,
            # `google_search` and `url_context` are executed by the model
            # server, not here. Leaving automatic function calling on makes the
            # SDK set up a local call loop this server has no functions for —
            # it warns about exactly that, and an enabled loop is a path by
            # which a grounded page could ask for a local call. Off.
            automatic_function_calling=types.AutomaticFunctionCallingConfig(
                disable=True
            ),
            http_options=types.HttpOptions(timeout=int(self.timeout_secs * 1000)),
        )
        response = self.client.models.generate_content(
            model=self.model,
            contents=prompt,
            config=config,
        )
        # Source count only: enough for an operator to see whether grounding
        # actually grounded, without putting the user's query or the answer on
        # a log that outlives the conversation.
        log.info("grounded response: %d source(s)", len(collect_sources(response)))
        rendered = render(response, max_chars=self.max_chars, show_queries=show_queries)
        return redact(rendered, self.secrets)


def make_client(settings: Any) -> Any:
    """Build the real `genai.Client` for these settings.

    Imported lazily so the offline suite never constructs one: the fake goes
    straight into `GroundedBackend`, and nothing in the test path touches
    credentials or auth libraries.
    """
    from google import genai

    if not settings.use_vertex:
        return genai.Client(api_key=settings.api_key)

    kwargs: dict[str, Any] = {"vertexai": True, "project": settings.project}
    if settings.location:
        kwargs["location"] = settings.location
    if settings.credentials_path:
        from google.oauth2 import service_account

        kwargs["credentials"] = service_account.Credentials.from_service_account_file(
            settings.credentials_path,
            scopes=["https://www.googleapis.com/auth/cloud-platform"],
        )
    return genai.Client(**kwargs)
