"""Library log chatter, silenced in one place.

Every server routes its stderr to Mooshik's diagnostics sink, and the sink
renders each line as a turn in the pane. So a third-party library that logs
at WARNING does not scroll past in a terminal, it becomes a message the user
reads as if Mooshik said it. `google_genai` emits an AFC style note on every
generate call, which is advice to the caller and not a problem to report.

The root logger already sits at WARNING in each server. That is not low
enough, because these libraries log the noise at WARNING itself. Pin them to
ERROR by name so a real failure still reaches the operator.
"""

from __future__ import annotations

import logging

#: Loggers whose routine output is advice rather than news.
QUIET_LOGGERS = (
    "google_genai",
    "google.adk",
    "google.auth",
    "google.generativeai",
    "httpx",
    "httpcore",
)


def quiet_library_logging(level: int = logging.ERROR) -> None:
    """Raise known-noisy third-party loggers to `level`.

    Called by each server after `basicConfig`. Ours are untouched, so
    `*_LOG_LEVEL` still controls what the server itself says.
    """
    for name in QUIET_LOGGERS:
        logging.getLogger(name).setLevel(level)
