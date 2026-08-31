"""The concept vocabulary, and the defensive parse of a model's JSON.

This is the contract two producers have to agree on. The ingester extracts
concepts from text; the artifacts server extracts them from screenshots and
recordings. Both write into the same graph through `lambo_derive`, so a type
one of them invents is a type the other's consumer will reject — the reason
this lives here rather than being copied.

The parse is deliberately forgiving about *shape* and strict about *content*:
models wrap JSON in prose and code fences, and that is not worth a retry, but
an unknown `concept_type` or an empty string is dropped rather than repaired.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

#: The five types Lambo accepts. Anything else is dropped, never coerced.
CONCEPT_TYPES = frozenset({"entity", "logic", "constraint", "resource", "observation"})

#: Lambo rejects strings over this length; clamp defensively.
MAX_CONCEPT_CHARS = 16_384

#: A single response yielding more than this is a runaway, not an extraction.
MAX_CONCEPTS_PER_CHUNK = 64


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
        concepts.append(
            Concept(content=content.strip()[:MAX_CONCEPT_CHARS], concept_type=concept_type)
        )
    return concepts
