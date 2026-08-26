"""The deterministic pipeline: walk → filter → scan → chunk → checkpoint → write.

The agent stays thin; this module owns every non-model decision:

* extension allowlist and repo metadata-only walking (`walker`),
* secret scan with whole-document drop (`secretscan`),
* chunking to budget (`chunker`),
* checkpoint resume (`checkpoint`),
* provenance: the source path rides into the graph as a `document:<source>`
  Resource — produced by a per-document `lambo_record_action`, and wired as
  the *parent* of each extracted concept through `lambo_derive`'s
  `parent_of` — so M9 can trace any extraction back to its document.
  Note: `lambo_derive` itself has no `produces` field on the wire (that is
  `lambo_record_action`'s); the brief's "derive produces resources" is
  realized as the pair of these two calls, which keeps the schema honest.

Delivery semantics are **at-least-once**: the checkpoint for a document is
marked only after its concepts are written, so a crash between the last
derive and the mark re-extracts and re-writes that document on the next run —
duplicates, never loss. Acceptable for a bootstrap loader (the graph
tolerates re-derives; M9 curation can merge), and a corrupt state file
degrades the same way: clean slate, full re-ingest. See the `checkpoint`
module docstring for recovery details.
"""

from __future__ import annotations

import asyncio
import hashlib
import logging
from dataclasses import dataclass, field

from . import walker
from .chunker import chunk_text
from .config import Settings
from .extraction import ConceptExtractor
from .secretscan import find_secret

log = logging.getLogger(__name__)

#: lambo caps one derive at 64 concepts (src/tools/schema.rs).
MAX_CONCEPTS_PER_DERIVE = 64


@dataclass
class DocumentReport:
    source: str
    status: str  # "written" | "dropped-secret" | "resumed" | ...
    concepts: int = 0


@dataclass
class Report:
    candidates: int = 0
    written: int = 0
    resumed: int = 0
    dropped: list[tuple[str, str]] = field(default_factory=list)
    concepts: int = 0
    derive_calls: int = 0
    action_calls: int = 0
    chunks: int = 0
    documents: list[DocumentReport] = field(default_factory=list)


def content_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def document_resource(source: str) -> str:
    """Resource name carrying provenance into the graph."""
    return f"document:{source}"


def _split_batches(concepts: list, size: int = MAX_CONCEPTS_PER_DERIVE):
    return [concepts[i : i + size] for i in range(0, len(concepts), size)] or [[]]


def plan(settings: Settings):
    """Walk + scan without touching Vertex or the graph. Returns (docs, report)."""
    from .checkpoint import DROPPED

    docs = walker.collect_documents(settings.root, settings.extensions)
    report = Report(candidates=len(docs))
    kept = []
    for doc in docs:
        hit = find_secret(doc.text, settings.extra_forbidden)
        if hit is None:
            kept.append(doc)
            continue
        report.dropped.append((doc.source, hit))
        report.documents.append(DocumentReport(doc.source, DROPPED))
        log.warning("dropped %s: matched pattern class %s", doc.source, hit)
    return kept, report


async def ingest(
    settings: Settings,
    writer,
    extractor: ConceptExtractor | None,
) -> Report:
    """Full run against a live writer and extractor (None ⇒ scan-only).

    Per document: derive + record-action first, `checkpoint.mark` last —
    the crash window in between yields at-least-once delivery (duplicates
    on resume), never loss. See the module docstring.
    """
    from .checkpoint import Checkpoint, DONE, DROPPED

    kept, report = plan(settings)
    checkpoint = Checkpoint(settings.state_path)
    for doc in kept:
        key = Checkpoint.key(doc.source, content_hash(doc.text))
        previous = checkpoint.status(key)
        if previous == DONE:
            report.resumed += 1
            report.documents.append(DocumentReport(doc.source, "resumed"))
            continue
        if previous == DROPPED:
            continue

        if settings.dry_run or extractor is None:
            report.documents.append(DocumentReport(doc.source, "scan-only"))
            continue

        concepts = []
        for chunk in chunk_text(doc.text, settings.chunk_chars):
            report.chunks += 1
            concepts.extend(extractor.extract(chunk))
        if not concepts:
            # Nothing extracted: still record the decision so re-runs skip it.
            checkpoint.mark(key, DONE)
            report.documents.append(
                DocumentReport(doc.source, "no-concepts")
            )
            continue

        parent = document_resource(doc.source)
        parent_of = [{"parent": parent, "child": c.content} for c in concepts]
        payload = [
            {"content": c.content, "concept_type": c.concept_type}
            for c in concepts
        ]
        for batch in _split_batches(payload):
            await writer.derive(settings.agent_id, batch, parent_of)
            report.derive_calls += 1
        await writer.record_action(
            settings.agent_id,
            f"Ingested {doc.kind} {doc.source}: "
            f"{len(concepts)} concepts extracted by {settings.model}",
            produces=[parent],
        )
        report.action_calls += 1
        report.concepts += len(concepts)
        report.written += 1
        report.documents.append(
            DocumentReport(doc.source, "written", len(concepts))
        )
        checkpoint.mark(key, DONE)
        log.info("wrote %d concepts from %s", len(concepts), doc.source)

    if not settings.dry_run and report.written:
        # Durability gate: the last derive acks before the embedder runs, so
        # tearing the child down now could discard the un-embedded tail (the
        # abrupt exit loses it — J3's warning). Hold until the child reports
        # an empty write-behind log.
        from .writer import drain

        if await drain(writer, settings.agent_id):
            log.info("write-behind log drained; writes are durable")
        else:
            log.warning("write-behind log did NOT drain in time; the last "
                        "writes may be lost")

    return report


def run_sync(settings: Settings, writer, extractor) -> Report:
    return asyncio.run(ingest(settings, writer, extractor))
