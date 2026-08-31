"""Process entry point: `python3 -m news_mcp`, or `python3 server.py`.

Startup order matters. Logging is bound to stderr *before* anything else can
emit a byte, then configuration is resolved and the client built. Only once
both have succeeded does `run("stdio")` claim stdin/stdout for JSON-RPC. A
failure before that point exits non-zero with a message on stderr and nothing
at all on stdout, so a half-configured server never looks to Mooshik like a
live one that answers badly.
"""

from __future__ import annotations

import logging
import sys

from .backend import GroundedBackend, make_client
from mooshik_common.logs import quiet_library_logging

from .config import ConfigError, Settings
from .tools import build_server

log = logging.getLogger("news_mcp")


def main(argv: list[str] | None = None) -> int:
    """Resolve the environment, build the server, serve stdio. Never prints."""
    argv = sys.argv[1:] if argv is None else argv
    if argv:
        # No CLI surface on purpose: a secret passed as an argument is visible
        # in `ps` and in shell history. Configuration is environment-only.
        print_stderr(
            "news_mcp takes no arguments; all configuration comes from the "
            "environment (see mcp-servers/news/README.md)."
        )
        return 2

    try:
        settings = Settings.from_env()
    except ConfigError as error:
        configure_logging("INFO")
        log.error("configuration error: %s", error)
        return 2

    configure_logging(settings.log_level)
    log.info("news MCP server starting: %s", settings.describe())

    try:
        client = make_client(settings)
    except Exception as error:  # noqa: BLE001 - fail closed with a named cause
        log.error("could not build the Google client (%s)", type(error).__name__)
        log.debug("client construction failed", exc_info=True)
        return 2

    backend = GroundedBackend(
        client,
        model=settings.model,
        max_chars=settings.max_chars,
        timeout_secs=settings.timeout_secs,
        secrets=(settings.api_key,) if settings.api_key else (),
    )
    build_server(backend, timeout_secs=settings.timeout_secs).run("stdio")
    return 0


LOG_FORMAT = "%(name)s %(levelname)s %(message)s"


def configure_logging(level: str) -> None:
    """Bind logging to stderr — never stdout, which carries JSON-RPC frames.

    The root logger stays at WARNING so the SDK's per-request INFO chatter does
    not drown the operator; only this package honours `NEWS_LOG_LEVEL`.
    """
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr, format=LOG_FORMAT)
    quiet_library_logging()
    try:
        log.setLevel(level)
    except ValueError:
        # An unrecognised NEWS_LOG_LEVEL is not worth refusing to start over.
        log.setLevel(logging.INFO)
        log.warning("unrecognised NEWS_LOG_LEVEL %r; using INFO", level)


def print_stderr(message: str) -> None:
    """The one place this process writes text by hand — and never to stdout."""
    sys.stderr.write(message + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
