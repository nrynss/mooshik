"""The milestone's ADK shape: a thin Gemini agent whose job is "given a chunk
of text, return concepts", with the MCP writer bridge as its tool surface.

Decision (see README §ADK-vs-genai and dev-diary/adversarial-review/
m8-implementation.md): the batch pipeline in `pipeline.py` drives
`google-genai` directly. ADK's Runner/session/event loop is built for
interactive multi-turn agents; M8 is a deterministic map over chunks with
checkpointing and rate limiting *between* calls, which a Runner can only
obscure (its session service wants to own history we deliberately do not
keep). This module keeps the letter of the ADK requirement: the same
instruction, the same model, and function tools that wrap the exact writer
seam (`lambo_derive` / `lambo_record_action`), so the agent is runnable
through any ADK Runner if a future interactive ingest mode wants it.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

from google.adk.agents import LlmAgent

from .config import DEFAULT_MODEL
from .extraction import PROMPT

#: The one instruction the agent carries: chunk in, concepts out.
INGEST_INSTRUCTION = PROMPT + (
    "\nAfter extracting, you may call record_concepts with the JSON array "
    "to persist them; the writer bridge routes to lambo_derive."
)

_writer: Any = None  # set by use_writer(); kept module-level so the
# function tools below stay plain callables as ADK requires.


def use_writer(writer: Any) -> None:
    """Point the agent's tools at a live LamboMcpWriter (or test fake)."""
    global _writer
    _writer = writer


def record_concepts(concepts_json: str) -> str:
    """ADK function tool: persist extracted concepts through the writer.

    Runs the async writer bridge from ADK's sync tool-call context.
    """
    if _writer is None:
        return json.dumps({"error": "no writer configured"})
    payload = json.loads(concepts_json)
    parent_of = [{"parent": c["source"], "child": c["content"]} for c in payload]
    derive = [
        {"content": c["content"], "concept_type": c["concept_type"]}
        for c in payload
    ]
    result = asyncio.run(_writer.derive("bootstrap", derive, parent_of))
    return json.dumps({"derive": result})


def build_agent(model: str = DEFAULT_MODEL) -> LlmAgent:
    """The bootstrap ingester as an ADK LlmAgent."""
    return LlmAgent(
        name="bootstrap_ingester",
        model=model,
        description=(
            "Walks machine history, extracts memory concepts from document "
            "chunks on Gemini Flash, writes them into Mooshik's graph."
        ),
        instruction=INGEST_INSTRUCTION,
        tools=[record_concepts],
    )
