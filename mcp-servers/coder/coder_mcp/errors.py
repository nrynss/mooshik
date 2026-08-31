"""What leaves this process: the one contained-failure type, and egress scrub.

Its own module so ``tools.py`` — the MCP surface — has no dependency on any
SDK. That keeps the import path of anything that only needs to shape a tool
result clean and fast to start.
"""

from __future__ import annotations

from typing import Sequence


class CoderToolError(Exception):
    """A failure with a message that is safe and useful to show the model.

    Bad arguments, a missing repo, an unknown handle: things the caller can
    fix. Everything else is an upstream failure whose detail stays on stderr.
    """


def redact(text: str, secrets: Sequence[str | None]) -> str:
    """Scrub known secret values out of anything leaving this process.

    Results flow into the companion's context and can be written into the
    user's long-term memory, so a credential echoed back inside an upstream
    error message would be persisted, not merely displayed. Mooshik redacts
    tool egress itself; this is the same guard one hop earlier, where the
    secret values are actually known.
    """
    for secret in secrets:
        if secret and secret in text:
            text = text.replace(secret, "[redacted]")
    return text
