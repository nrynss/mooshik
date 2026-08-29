"""Pool definitions — the population each measurement is taken over.

Decisions (recorded in dev-diary/adversarial-review/m9-implementation.md):

* **raw pool** — concepts whose provenance ties them to an ingested document:
  targets of ``Hierarchical`` edges whose source concept's content starts with
  ``document:`` (the M8 ingester wires every extracted concept as a child of
  its ``document:<source>`` resource via ``lambo_derive``'s ``parent_of``).
* **canonical pool** — ``canonization_status = 'Canonical'``. The enum lives in
  lambo ``src/types/mod.rs``: ``None | Candidate | Venerable | Canonical``.
* **wrong-rejection pool** — raw-pool concepts that were *not* promoted, i.e.
  status in ``(None, Candidate, Venerable)``. Lambo has no explicit
  ``Rejected`` status: budget demotions land on ``None``, unevaluated concepts
  sit at ``None``/``Candidate``/``Venerable``. Every extracted fact that is not
  Canonical today was implicitly rejected for promotion, so the whole
  non-canonical slice of the raw pool is the gradeable wrong-rejection
  population.
"""

from __future__ import annotations

from typing import Any

# Literal '%' inside a LIKE pattern must be doubled under psycopg's %s style.
RAW_POOL_SQL = """
SELECT DISTINCT c.id::text  AS node_id,
       c.content             AS content,
       c.concept_type        AS concept_type,
       c.canonization_status AS status,
       c.created_at          AS created_at,
       d.content             AS source_ref,
       (c.embedding IS NOT NULL) AS embedded
FROM concepts c
JOIN edges e    ON e.session_id = c.session_id
               AND e.target = c.id
               AND e.edge_type = 'Hierarchical'
JOIN concepts d ON d.session_id = c.session_id
               AND d.id = e.source
WHERE c.session_id = %s
  AND d.content LIKE 'document:%%'
ORDER BY c.id
"""

REJECTED_POOL_SQL = """
SELECT DISTINCT c.id::text  AS node_id,
       c.content             AS content,
       c.concept_type        AS concept_type,
       c.canonization_status AS status,
       c.created_at          AS created_at,
       d.content             AS source_ref,
       (c.embedding IS NOT NULL) AS embedded
FROM concepts c
JOIN edges e    ON e.session_id = c.session_id
               AND e.target = c.id
               AND e.edge_type = 'Hierarchical'
JOIN concepts d ON d.session_id = c.session_id
               AND d.id = e.source
WHERE c.session_id = %s
  AND d.content LIKE 'document:%%'
  AND c.canonization_status <> 'Canonical'
ORDER BY c.id
"""

CANONICAL_POOL_SQL = """
SELECT id::text              AS node_id,
       content               AS content,
       concept_type          AS concept_type,
       canonization_status   AS status,
       created_at            AS created_at,
       NULL::text            AS source_ref,
       (embedding IS NOT NULL) AS embedded
FROM concepts
WHERE session_id = %s
  AND canonization_status = 'Canonical'
ORDER BY id
"""

#: Coverage over the **extraction population** — the same set `RAW_POOL_SQL`
#: samples, and the same set a query can return.
#:
#: Counting every concept in the session was wrong, and wrong by a fixed
#: amount rather than noisily. The ingester writes two provenance nodes per
#: document (a `document:<src>` anchor and an "Ingested ..." action concept)
#: and neither is ever embedded, because neither is knowledge anybody would
#: want recalled. With `c` extracted concepts per document the bias is
#:
#:     2 / (c + 2)
#:
#: — independent of corpus size, and 14.2% on the first clean run (c = 12.1).
#: A 90% gate therefore fires on ANY corpus averaging fewer than ~18 concepts
#: per document, however healthy the graph, and hardest on short documents
#: where a trustworthy signal matters most. Lowering the threshold only moves
#: the arbitrary number; the denominator was the defect.
#:
#: Note this classifies by STRUCTURE, not by name: the anchors are created as
#: `Entity`, the same type as genuine Entity concepts, so neither
#: `concept_type` nor a `content` prefix on the measured node can separate
#: them. The prefix here identifies the join's *source* only, exactly as the
#: raw pool already does.
COVERAGE_SQL = """
SELECT count(*)          AS total,
       count(embedding)  AS embedded
FROM (
    SELECT DISTINCT c.id, c.embedding
    FROM concepts c
    JOIN edges e    ON e.session_id = c.session_id
                   AND e.target = c.id
                   AND e.edge_type = 'Hierarchical'
    JOIN concepts d ON d.session_id = c.session_id
                   AND d.id = e.source
    WHERE c.session_id = %s
      AND d.content LIKE 'document:%%'
) extracted
"""


def _dedupe_by_node_id(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Collapse join fan-out to one row per concept.

    DISTINCT removes fully identical rows, but a concept with two
    ``document:*`` parents (same content extracted from two documents gains a
    second Hierarchical edge) yields two rows differing only in
    ``source_ref``. Keep the first occurrence per node_id; order stays stable
    because the SQL orders by id.
    """
    unique: dict[str, dict[str, Any]] = {}
    for row in rows:
        unique.setdefault(str(row["node_id"]), row)
    return list(unique.values())


def raw_pool(conn: Any, session: str) -> list[dict[str, Any]]:
    return _dedupe_by_node_id(conn.query(RAW_POOL_SQL, (session,)))


def canonical_pool(conn: Any, session: str) -> list[dict[str, Any]]:
    return conn.query(CANONICAL_POOL_SQL, (session,))


def rejected_pool(conn: Any, session: str) -> list[dict[str, Any]]:
    return _dedupe_by_node_id(conn.query(REJECTED_POOL_SQL, (session,)))


def coverage_overall(conn: Any, session: str) -> tuple[int, int]:
    """(embedded, total) durable-embedding counts over the EXTRACTION population.

    Not every concept in the session: provenance nodes are never embedded by
    design, so including them puts a floor under the metric that no healthy
    graph can clear. See `COVERAGE_SQL`.
    """
    row = conn.query(COVERAGE_SQL, (session,))[0]
    return int(row["embedded"]), int(row["total"])
