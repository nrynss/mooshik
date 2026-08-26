"""Secret scanner — rule 2 (non-negotiable).

Every candidate document is scanned before chunking. A single hit DROPS the
whole document; the scanner never redacts and its logs carry the file path
and the *pattern class name* only, never matched content.

Pattern classes:
* PEM / certificate blocks
* AWS access keys (`AKIA…`)
* GitHub tokens (`ghp_…`, `gho_…`, …, `github_pat_…`)
* Slack tokens (`xox…`)
* generic high-entropy assignments
  (`SECRET|TOKEN|PASSWORD|API_KEY` = long base64-ish literal)
* every value passed via `INGEST_EXTRA_FORBIDDEN` (the Mooshik vault's
  current values, handed in by the operator — vault values never leave the
  Rust side, so they arrive as a comma-separated env list).
"""

from __future__ import annotations

import re

_PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    (
        "pem-block",
        re.compile(
            r"-----BEGIN [A-Z0-9 ]*(PRIVATE KEY|CERTIFICATE|CERTIFICATE REQUEST|"
            r"ENCRYPTED PRIVATE KEY|OPENSSH PRIVATE KEY|EC PRIVATE KEY|"
            r"DSA PRIVATE KEY|PGP PRIVATE KEY BLOCK)[A-Z0-9 ]*-----"
        ),
    ),
    ("aws-access-key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    (
        "github-token",
        re.compile(r"\bgh[posur]_[A-Za-z0-9]{36,255}\b"),
    ),
    ("github-pat", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{22,255}\b")),
    ("slack-token", re.compile(r"\bxox[abprsce]-[A-Za-z0-9-]{10,250}\b")),
    (
        "generic-assignment",
        re.compile(
            r"(?i)\b(SECRET|TOKEN|PASSWORD|PASSPHRASE|API_?KEY)"
            r"\b['\"]?\s*[:=]\s*['\"]?[A-Za-z0-9+/=_\-]{20,}"
        ),
    ),
)


def find_secret(text: str, extra_forbidden: tuple[str, ...] = ()) -> str | None:
    """Return the pattern-class name of the first hit, or `None`.

    The return value names a pattern class, never content. Extra forbidden
    vault values are reported as the class `vault-value`.
    """
    for name, pattern in _PATTERNS:
        if pattern.search(text):
            return name
    for value in extra_forbidden:
        if value and value in text:
            return "vault-value"
    return None
