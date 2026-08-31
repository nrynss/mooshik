"""Environment-driven settings for the bootstrap ingester.

Every knob has a default so `python3 -m ingester --root <dir>` works bare;
production overrides arrive through the environment (Cloud Run env vars map
one-to-one onto these names).
"""

from __future__ import annotations

import os
import socket
from dataclasses import dataclass, field
from pathlib import Path

from mooshik_common.models import DEFAULT_LOCATION as COMMON_LOCATION
from mooshik_common.models import DEFAULT_MODEL as COMMON_MODEL

DEFAULT_EXTENSIONS = ".md,.markdown,.txt,.rst"
DEFAULT_LAMBO_SERVE = "lambo serve"
#: Inference model and region both come from `mooshik_common.models`, which
#: carries the reasoning: 3.x is served only from `global`, and
#: MOOSHIK_GEMINI_LOCATION is the embedder's variable and must not be read
#: here. `INGEST_LOCATION` overrides the region for this component alone.
DEFAULT_MODEL = COMMON_MODEL
DEFAULT_LOCATION = COMMON_LOCATION
DEFAULT_CHUNK_CHARS = 4_000
DEFAULT_SLEEP_SECS = 0.5
DEFAULT_MAX_ATTEMPTS = 4
DEFAULT_STATE = ".ingest/state.json"


def _split_csv(raw: str) -> tuple[str, ...]:
    return tuple(item.strip() for item in raw.split(",") if item.strip())


@dataclass(frozen=True)
class Settings:
    """One resolved ingester run."""

    root: Path
    dry_run: bool = False
    extensions: tuple[str, ...] = (
        ".md",
        ".markdown",
        ".txt",
        ".rst",
    )
    extra_forbidden: tuple[str, ...] = ()
    session: str = field(default_factory=lambda: f"ingest-{socket.gethostname()}")
    agent_id: str = "bootstrap"
    lambo_serve: str = DEFAULT_LAMBO_SERVE
    state_path: Path = Path(DEFAULT_STATE)
    model: str = DEFAULT_MODEL
    project: str | None = None
    location: str | None = None
    credentials_path: str | None = None
    chunk_chars: int = DEFAULT_CHUNK_CHARS
    sleep_secs: float = DEFAULT_SLEEP_SECS
    max_attempts: int = DEFAULT_MAX_ATTEMPTS

    @classmethod
    def from_env(cls, root: Path, dry_run: bool = False) -> "Settings":
        root = root.resolve()
        state_raw = os.environ.get("INGEST_STATE", DEFAULT_STATE)
        state_path = Path(state_raw)
        if not state_path.is_absolute():
            state_path = root / state_path
        return cls(
            root=root,
            dry_run=dry_run,
            extensions=_split_csv(
                os.environ.get("INGEST_EXTENSIONS", DEFAULT_EXTENSIONS)
            )
            or tuple(DEFAULT_EXTENSIONS.split(",")),
            extra_forbidden=_split_csv(os.environ.get("INGEST_EXTRA_FORBIDDEN", "")),
            session=os.environ.get("INGEST_SESSION")
            or f"ingest-{socket.gethostname()}",
            agent_id=os.environ.get("INGEST_AGENT", "bootstrap"),
            lambo_serve=os.environ.get("INGEST_LAMBO_SERVE", DEFAULT_LAMBO_SERVE),
            state_path=state_path,
            model=os.environ.get("INGEST_MODEL", DEFAULT_MODEL),
            project=os.environ.get("MOOSHIK_GEMINI_PROJECT") or None,
            location=os.environ.get("INGEST_LOCATION") or DEFAULT_LOCATION,
            credentials_path=os.environ.get("MOOSHIK_GEMINI_CREDENTIALS") or None,
            chunk_chars=int(os.environ.get("INGEST_CHUNK_CHARS", DEFAULT_CHUNK_CHARS)),
            sleep_secs=float(os.environ.get("INGEST_SLEEP_SECS", DEFAULT_SLEEP_SECS)),
            max_attempts=int(os.environ.get("INGEST_MAX_ATTEMPTS", DEFAULT_MAX_ATTEMPTS)),
        )
