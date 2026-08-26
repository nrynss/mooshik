"""Seeded sampling of measurement pools and jsonl persistence.

Determinism: each pool draws from its own ``random.Random`` derived from the
base seed (``f"{seed}:{pool}"``), so adding ``--rejected N`` later never
perturbs an earlier raw/canonical draw, and same seed -> same sample.
``random.Random.sample`` on a deterministically ordered pool (SQL ``ORDER BY
id``) is a without-replacement draw: no duplicates within a pool, and ``N``
larger than the pool clamps to the pool size. The rejected draw excludes node
ids already drawn into the raw sample so no item is graded twice.
"""

from __future__ import annotations

import json
import random
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

POOLS = ("raw", "canonical", "rejected")

VERDICTS = ("correct", "incorrect", "unsure")


@dataclass
class SampleItem:
    pool: str
    node_id: str
    content: str
    concept_type: str
    status: str
    created_at: str
    sources: list[str] = field(default_factory=list)
    embedded: bool = False


def _to_item(pool: str, row: dict[str, Any]) -> SampleItem:
    return SampleItem(
        pool=pool,
        node_id=str(row["node_id"]),
        content=str(row["content"]),
        concept_type=str(row["concept_type"]),
        status=str(row["status"]),
        created_at=str(row["created_at"]),
        sources=[str(row["source_ref"])] if row.get("source_ref") else [],
        embedded=bool(row["embedded"]),
    )


def draw(
    pool: str,
    rows: list[dict[str, Any]],
    n: int,
    seed: int,
    exclude: set[str] | None = None,
) -> list[SampleItem]:
    """Deterministic without-replacement draw; N > len(rows) clamps."""
    candidates = [r for r in rows if exclude is None or str(r["node_id"]) not in exclude]
    rng = random.Random(f"{seed}:{pool}")
    picked = rng.sample(range(len(candidates)), k=min(n, len(candidates)))
    return [_to_item(pool, candidates[i]) for i in sorted(picked)]


def run_sampling(
    conn: Any,
    session: str,
    n_raw: int,
    n_canonical: int,
    n_rejected: int,
    seed: int,
) -> tuple[list[SampleItem], dict[str, int]]:
    """Sample all pools; returns items plus the true pool sizes observed."""
    from . import pools

    raw_rows = pools.raw_pool(conn, session)
    canon_rows = pools.canonical_pool(conn, session)
    rejected_rows = pools.rejected_pool(conn, session)

    sizes = {
        "raw": len(raw_rows),
        "canonical": len(canon_rows),
        "rejected": len(rejected_rows),
    }
    raw_items = draw("raw", raw_rows, n_raw, seed)
    canon_items = draw("canonical", canon_rows, n_canonical, seed)
    drawn_ids = {it.node_id for it in raw_items} | {it.node_id for it in canon_items}
    rejected_items = draw("rejected", rejected_rows, n_rejected, seed, exclude=drawn_ids)

    return raw_items + canon_items + rejected_items, sizes


# ---------------------------------------------------------------- jsonl io


def write_jsonl(items: list[SampleItem], path: Path) -> None:
    with path.open("w", encoding="utf-8") as fh:
        for item in items:
            fh.write(json.dumps(asdict(item), ensure_ascii=False) + "\n")


def read_jsonl(path: Path) -> list[SampleItem]:
    items: list[SampleItem] = []
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                rec = json.loads(line)
                items.append(SampleItem(**rec))
    return items
