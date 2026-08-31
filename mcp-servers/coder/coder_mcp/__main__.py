"""Process entry point: ``python3 -m coder_mcp``, or ``python3 server.py``.

Startup order matters. Logging is bound to stderr *before* anything else can
emit a byte, then configuration is resolved and the backend built. Only once
both have succeeded does ``run("stdio")`` claim stdin/stdout for JSON-RPC. A
failure before that point exits non-zero with a message on stderr and nothing
at all on stdout, so a half-configured server never looks to Mooshik like a
live one that answers badly.

Unlike the news server, this one has no Google SDK client to construct — the
backend is pure subprocess management. Configuration still fails closed: a
missing or invalid ``MOOSHIK_CODER_AGENT`` is a hard exit 2.
"""

from __future__ import annotations

import logging
import sys

from .backend import CoderBackend
from .config import ConfigError, Settings
from .tools import build_server

log = logging.getLogger("coder_mcp")


def main(argv: list[str] | None = None) -> int:
    """Resolve the environment, build the server, serve stdio. Never prints."""
    argv = sys.argv[1:] if argv is None else argv

    # `--agent` is the ONE argument this server accepts, and the rule it bends
    # is worth restating rather than deleting: a secret passed as an argument
    # is visible in `ps` and in shell history, so every secret still comes from
    # the environment and only from there. An agent name is not a secret — and
    # it cannot travel by environment anyway, because `[mcp_servers.*.env]` is
    # resolved as vault secret NAMES, so a literal there is looked up as a
    # secret, not found, and the server never spawns.
    agent_override: str | None = None
    rest = list(argv)
    if rest and rest[0].startswith("--agent"):
        head = rest.pop(0)
        if "=" in head:
            agent_override = head.split("=", 1)[1]
        elif rest:
            agent_override = rest.pop(0)
        else:
            print_stderr("--agent requires a value.")
            return 2
    if rest:
        print_stderr(
            "coder_mcp takes no arguments except --agent; every other setting, "
            "and every secret, comes from the environment "
            "(see mcp-servers/coder/README.md)."
        )
        return 2

    try:
        settings = Settings.from_env(agent_override=agent_override)
    except ConfigError as error:
        configure_logging("INFO")
        log.error("configuration error: %s", error)
        return 2

    configure_logging(settings.log_level)
    log.info("coder MCP server starting: %s", settings.describe())

    # Build the env dict that the agent subprocess will inherit. Only variables
    # that have been explicitly passed through to this process are forwarded —
    # the rest is set by `agents.py` from the agent's own needs.
    passthrough_env: dict[str, str] = {}
    if settings.anthropic_api_key:
        passthrough_env["ANTHROPIC_API_KEY"] = settings.anthropic_api_key
    if settings.gemini_api_key:
        passthrough_env["MOOSHIK_GEMINI_API_KEY"] = settings.gemini_api_key
    if settings.gemini_project:
        passthrough_env["MOOSHIK_GEMINI_PROJECT"] = settings.gemini_project
    if settings.cursor_api_key:
        passthrough_env["CURSOR_API_KEY"] = settings.cursor_api_key

    backend = CoderBackend(agent=settings.agent, env=passthrough_env)
    build_server(backend, timeout_secs=settings.timeout_secs).run("stdio")
    return 0


LOG_FORMAT = "%(name)s %(levelname)s %(message)s"


def configure_logging(level: str) -> None:
    """Bind logging to stderr — never stdout, which carries JSON-RPC frames.

    The root logger stays at WARNING so third-party library chatter does not
    drown the operator; only this package honours ``CODER_LOG_LEVEL``.
    """
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr, format=LOG_FORMAT)
    try:
        log.setLevel(level)
    except ValueError:
        # An unrecognised CODER_LOG_LEVEL is not worth refusing to start over.
        log.setLevel(logging.INFO)
        log.warning("unrecognised CODER_LOG_LEVEL %r; using INFO", level)


def print_stderr(message: str) -> None:
    """The one place this process writes text by hand — and never to stdout."""
    sys.stderr.write(message + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
