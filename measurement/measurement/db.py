"""SQL access seam.

Every graph read goes through :class:`Connection`, a one-method protocol
returning plain dict rows. The real implementation wraps psycopg with a DSN
from the environment; tests fake it (see ``tests/test_measurement.py``), so
nothing in this package touches a network unless explicitly run live.

psycopg is imported lazily inside :class:`PgConnection` so the offline test
suite and CI need only pytest installed.
"""

from __future__ import annotations

import os
from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class Connection(Protocol):
    """One method, dict rows out. That is the whole contract."""

    def query(self, sql: str, params: tuple[Any, ...] = ()) -> list[dict[str, Any]]:
        ...


DEFAULT_DSN_ENV = "MOOSHIK_POSTGRES_DSN"
DEFAULT_SESSION = "mooshik"


def dsn_from_env(env: dict[str, str] | None = None) -> str:
    source = os.environ if env is None else env
    dsn = source.get(DEFAULT_DSN_ENV, "").strip()
    if not dsn:
        raise SystemExit(
            f"error: {DEFAULT_DSN_ENV} is not set — source the worktree .env "
            "or pass --dsn"
        )
    return dsn


class PgConnection:
    """Real Connection over psycopg. One short-lived connection per query —
    the harness issues a handful of reads per invocation and holds no state."""

    def __init__(self, dsn: str) -> None:
        self._dsn = dsn

    def query(self, sql: str, params: tuple[Any, ...] = ()) -> list[dict[str, Any]]:
        import psycopg
        from psycopg.rows import dict_row

        with psycopg.connect(self._dsn, row_factory=dict_row) as conn, conn.cursor() as cur:
            cur.execute(sql, params or None)
            rows: list[dict[str, Any]] = cur.fetchall()  # type: ignore[assignment]
            return [dict(r) for r in rows]
