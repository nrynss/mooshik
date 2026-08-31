"""Gemini Flash extraction over Vertex AI.

One chunk in, zero-or-more concepts out, as strict JSON:

    [{"content": "...", "concept_type": "entity|logic|constraint|resource|observation"}]

Defensive parsing: code fences are stripped, entries with unknown types or
empty content are discarded, unparseable chunks get exactly one retry before
being skipped (the skip is counted and logged). 429 responses back off
exponentially up to ``max_attempts``.

Decision (documented in README + dev-diary): the batch pipeline drives
`google-genai` directly rather than through an ADK Runner — M8's loop is a
deterministic map over chunks with checkpointing between calls, which ADK's
session/runner abstractions only obscure. `ingester.agent` keeps the
milestone's ADK shape.
"""

from __future__ import annotations

import json
import logging
import re
import time
from dataclasses import dataclass

from google import genai

from .config import DEFAULT_MODEL

log = logging.getLogger(__name__)

CONCEPT_TYPES = frozenset(
    {"entity", "logic", "constraint", "resource", "observation"}
)

#: Lambo rejects strings over this length; clamp defensively.
MAX_CONCEPT_CHARS = 16_384
MAX_CONCEPTS_PER_CHUNK = 64

PROMPT = (
    "You extract durable memory concepts from workspace text.\n"
    "Given one chunk of a document, return ZERO OR MORE concepts worth "
    "remembering long-term, as a strict JSON array and nothing else:\n"
    '[{"content": "<one self-contained concept>", '
    '"concept_type": "entity|logic|constraint|resource|observation"}]\n'
    "Rules: no prose around the JSON; no duplicates; skip trivia, greetings "
    "and boilerplate; each content string must stand alone without referring "
    "to 'this document' or 'the chunk'. Return [] when nothing qualifies."
)


@dataclass(frozen=True)
class Concept:
    content: str
    concept_type: str


def parse_concepts(raw: str) -> list[Concept]:
    """Parse model output defensively into valid concepts."""
    text = raw.strip()
    fenced = re.search(r"```(?:json)?\s*(.*?)\s*```", text, re.DOTALL)
    if fenced:
        text = fenced.group(1)
    start, end = text.find("["), text.rfind("]")
    if start == -1 or end == -1 or end <= start:
        raise ValueError("no JSON array in response")
    payload = json.loads(text[start : end + 1])
    if not isinstance(payload, list):
        raise ValueError("response is not a JSON array")
    concepts: list[Concept] = []
    for entry in payload[:MAX_CONCEPTS_PER_CHUNK]:
        if not isinstance(entry, dict):
            continue
        content = entry.get("content")
        concept_type = entry.get("concept_type")
        if not isinstance(content, str) or not content.strip():
            continue
        if concept_type not in CONCEPT_TYPES:
            continue
        concepts.append(Concept(content=content.strip()[:MAX_CONCEPT_CHARS], concept_type=concept_type))
    return concepts


def make_client(
    project: str | None,
    location: str | None,
    credentials_path: str | None,
) -> genai.Client:
    """Vertex-mode client using the service account json from the env."""
    kwargs: dict[str, object] = {}
    if credentials_path:
        from google.oauth2 import service_account

        credentials = service_account.Credentials.from_service_account_file(
            credentials_path,
            scopes=["https://www.googleapis.com/auth/cloud-platform"],
        )
        kwargs["credentials"] = credentials
    if project:
        kwargs["project"] = project
    if location:
        kwargs["location"] = location
    kwargs["vertexai"] = True
    return genai.Client(**kwargs)


class ConceptExtractor:
    """Chunk → concepts on Gemini Flash, with retry/backoff/skip accounting."""

    def __init__(
        self,
        client: object,
        model: str = DEFAULT_MODEL,
        sleep_secs: float = 0.5,
        max_attempts: int = 4,
        clock=time.sleep,
    ):
        self.client = client
        self.model = model
        self.sleep_secs = sleep_secs
        self.max_attempts = max(1, max_attempts)
        self.clock = clock
        self.skipped_chunks = 0
        self.calls = 0

    def extract(self, chunk: str) -> list[Concept]:
        """Extract concepts from one chunk; skips after one failed parse."""
        for attempt in range(self.max_attempts):
            self.calls += 1
            try:
                response = self.client.models.generate_content(
                    model=self.model,
                    contents=f"{PROMPT}\n\nCHUNK:\n{chunk}",
                )
                raw = response.text or ""
            except Exception as error:  # noqa: BLE001 - see _is_rate_limit
                if _is_rate_limit(error) and attempt < self.max_attempts - 1:
                    delay = 2**attempt * max(self.sleep_secs, 1.0)
                    log.warning(
                        "rate limited by Vertex; backing off %.1fs", delay
                    )
                    self.clock(delay)
                    continue
                raise
            self.clock(self.sleep_secs)
            try:
                return parse_concepts(raw)
            except ValueError:
                if attempt >= 1:
                    break
                log.warning("unparseable extraction output; retrying once")
        self.skipped_chunks += 1
        log.warning("chunk skipped after retry: still unparseable")
        return []


def _is_rate_limit(error: Exception) -> bool:
    text = f"{type(error).__name__}: {error}"
    return "429" in text or "RESOURCE_EXHAUSTED" in text
