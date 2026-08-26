"""Resolve a ``document:<source>`` provenance ref to a human-readable excerpt.

Recognized refs (as written by the M8 ingester's ``lambo_record_action``):

* ``document:file:<path>``        — head of the file at ``<path>``
* ``document:git:<path>#<sha>``   — the commit object (subject + body) via
  ``git -C <path> show --no-patch <sha>``; commit metadata is all the ingester
  ever extracted from repos, so the message *is* the source text.

Anything else, or an unavailable path/repo, resolves to None — grading then
shows "(source not resolvable)" and the human decides from context.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

EXCERPT_CHARS = 700


def _head(path: str, limit: int) -> str | None:
    try:
        text = Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    return text[:limit]


def _git_commit(repo: str, sha: str, limit: int) -> str | None:
    if not Path(repo).is_dir():
        return None
    try:
        out = subprocess.run(
            ["git", "-C", repo, "show", "--no-patch", "--format=fuller", sha],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout[:limit] or None


def resolve_excerpt(source_ref: str, limit: int = EXCERPT_CHARS) -> str | None:
    """Excerpt for one ref, or None when it cannot be resolved."""
    if not source_ref.startswith("document:"):
        return None
    rest = source_ref[len("document:") :]
    if rest.startswith("file:"):
        return _head(rest[len("file:") :], limit)
    if rest.startswith("git:"):
        spec = rest[len("git:") :]
        repo, sep, sha = spec.partition("#")
        return _git_commit(repo, sha, limit) if sep and sha else None
    return None


def excerpt_for(sources: list[str], limit: int = EXCERPT_CHARS) -> str:
    for ref in sources:
        found = resolve_excerpt(ref, limit)
        if found:
            return found.strip()
    return "(source not resolvable: " + ("; ".join(sources) if sources else "no ref") + ")"
