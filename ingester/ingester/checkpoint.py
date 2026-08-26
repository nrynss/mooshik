"""Checkpoint state so re-runs resume and never re-extract.

State lives in `.ingest/state.json` (gitignored), keyed by
`(source_path, content_hash)` — a re-run of unchanged content is skipped even
if the file moved; changed content is re-extracted under its own key.
"""

from __future__ import annotations

import json
import os
import tempfile
from datetime import datetime, timezone
from pathlib import Path

DONE = "done"
DROPPED = "dropped-secret"


class Checkpoint:
    """Persistent record of per-document ingest decisions."""

    def __init__(self, path: Path):
        self.path = path
        self._done: dict[str, str] = {}
        self._load()

    def _load(self) -> None:
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return
        except (OSError, ValueError):
            # A corrupt state file must not wedge the pipeline forever: start
            # over rather than crash every run.
            return
        done = raw.get("done", {})
        if isinstance(done, dict):
            self._done = {str(k): str(v) for k, v in done.items()}

    @staticmethod
    def key(source: str, content_hash: str) -> str:
        return f"{source}::{content_hash}"

    def status(self, key: str) -> str | None:
        return self._done.get(key)

    def mark(self, key: str, status: str = DONE) -> None:
        self._done[key] = status
        self.save()

    def save(self) -> None:
        payload = {"done": self._done}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        handle = tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=self.path.parent,
            prefix=".state-",
            suffix=".tmp",
            delete=False,
        )
        try:
            with handle:
                json.dump(payload, handle, indent=1, sort_keys=True)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(handle.name, self.path)
        except BaseException:
            try:
                os.unlink(handle.name)
            except OSError:
                pass
            raise

    @staticmethod
    def now_iso() -> str:
        return datetime.now(timezone.utc).isoformat(timespec="seconds")
