from __future__ import annotations
from typing import Sequence

class ArtifactsToolError(Exception): pass

def redact(text: str, secrets: Sequence[str | None]) -> str:
    for secret in secrets:
        if secret and secret in text:
            text = text.replace(secret, "[redacted]")
    return text
