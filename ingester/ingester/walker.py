"""Corpus discovery: extension-allowlisted files and git commit metadata.

Rule 1 (non-negotiable): the allowlist is the *only* file filter. There is no
denylist anywhere in the pipeline.

Rule 3 (non-negotiable): when the walk reaches a directory that is a git
repository (a `.git` entry), that directory contributes **commit metadata
only** — hash, author date, subject + body via `git log --format`. The walker
never descends into a repository for its working-tree files, so patch/diff
content cannot enter the corpus by construction.
"""

from __future__ import annotations

import logging
import subprocess
from dataclasses import dataclass
from pathlib import Path

log = logging.getLogger(__name__)

#: Directories never descended into, whatever the allowlist says.
SKIP_DIRS = frozenset(
    {
        ".git",
        ".ingest",
        ".venv",
        "venv",
        "node_modules",
        "target",
        "__pycache__",
        ".pytest_cache",
    }
)

#: `git log` record format: NUL-separated fields, RS-terminated records.
#: Fields are hash, author date (ISO-8601), raw body (`%B` = subject + body).
#: Deliberately NO `%P`-adjacent patch placeholders: there is no way to ask
#: this format for a diff, which is the property rule 3 wants.
_GIT_LOG_FORMAT = "%H%x00%aI%x00%B%x1e"


@dataclass(frozen=True)
class Document:
    """One unit of ingestable text."""

    #: Stable provenance id: absolute path for files, `git:<repo>#<sha>` for
    #: commits. Becomes the M9-traceable resource name downstream.
    source: str
    path: Path | None
    kind: str  # "file" | "commit"
    text: str


def iter_files(root: Path, extensions: tuple[str, ...]) -> list[Path]:
    """Allowlisted files under ``root``, skipping repos' metadata-only dirs.

    Symbolic links are never collected — neither files nor directories.
    ``rglob`` does not recurse into directory symlinks, but it *does* yield
    file symlinks, and following one would ingest content living outside
    the corpus root. The root directory is the trust boundary; a symlink
    crossing it is rejected on sight.
    """
    allowed = {ext.lower() for ext in extensions}
    found: list[Path] = []
    stack = [root]
    while stack:
        current = stack.pop()
        if not current.is_dir():
            continue
        for entry in sorted(current.iterdir()):
            if not entry.is_dir():
                continue
            if entry.name in SKIP_DIRS:
                continue
            if (entry / ".git").exists():
                continue  # repositories are handled by iter_commits only
            stack.append(entry)
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            continue  # never cross the corpus-root boundary via a link
        if not path.is_file():
            continue
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if _in_repo(root, path):
            continue  # inside a repository: metadata-only walking (rule 3)
        if path.suffix.lower() not in allowed:
            continue
        found.append(path)
    return sorted(set(found))


def _in_repo(root: Path, path: Path) -> bool:
    """True when any ancestor of ``path`` below ``root`` holds a `.git`."""
    rel = path.relative_to(root).parts[:-1]
    current = root
    for part in rel:
        current = current / part
        if (current / ".git").exists():
            return True
    return False


def iter_repos(root: Path) -> list[Path]:
    """Directories at or under ``root`` that are git repositories."""
    repos: list[Path] = []
    if (root / ".git").exists():
        repos.append(root)
    stack = [root]
    while stack:
        current = stack.pop()
        if not current.is_dir():
            continue
        for entry in sorted(current.iterdir()):
            if not entry.is_dir() or entry.name in SKIP_DIRS:
                continue
            if (entry / ".git").exists():
                repos.append(entry)
            else:
                stack.append(entry)
    return sorted(set(repos))


def iter_commits(repo: Path) -> list[Document]:
    """Commit-metadata documents for one repository.

    Each record carries hash, author date, subject + body. Never patches.
    """
    try:
        proc = subprocess.run(
            ["git", "-C", str(repo), "log", f"--format={_GIT_LOG_FORMAT}"],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        log.warning("skipping repo %s: %s", repo, error)
        return []
    docs: list[Document] = []
    for record in proc.stdout.split("\x1e"):
        record = record.strip()
        if not record:
            continue
        fields = record.split("\x00")
        if len(fields) != 3:
            log.warning("malformed commit record in %s; skipped", repo)
            continue
        sha, author_date, body = (field.strip() for field in fields)
        docs.append(
            Document(
                source=f"git:{repo}#{sha}",
                path=repo,
                kind="commit",
                text=f"commit {sha}\nauthor-date {author_date}\n{body}",
            )
        )
    return docs


def collect_documents(root: Path, extensions: tuple[str, ...]) -> list[Document]:
    """The full candidate corpus: allowlisted files plus commit metadata."""
    docs: list[Document] = []
    for path in iter_files(root, extensions):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            log.warning("unreadable %s: %s", path, error)
            continue
        docs.append(
            Document(
                source=f"file:{path}",
                path=path,
                kind="file",
                text=text,
            )
        )
    for repo in iter_repos(root):
        docs.extend(iter_commits(repo))
    return docs
