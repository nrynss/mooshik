"""Environment-driven settings for the coder MCP server.

Mooshik spawns this server as a child process over stdio and injects the
credentials it needs as **environment variables**, resolved from its encrypted
vault at spawn time (``src/mcp_host/mod.rs``). Everything here follows from
that contract:

* All configuration comes from the environment. No config file is read, and no
  secret is ever accepted as a CLI argument, where it would land in ``ps``
  output and shell history.
* A missing ``MOOSHIK_CODER_AGENT`` fails closed, naming the variable. Nothing
  about a *value* is ever logged or put in an error message — only whether a
  name was set, so a stderr transcript is safe to paste into a bug report.

The agent choice — ``claude``, ``omp``, ``cursor``, or ``agy`` — decides which CLI
binary is spawned and which credential variables are relevant. Credentials
for an agent not selected are silently ignored, which keeps the config block
universal: you can set all API keys and ``MOOSHIK_CODER_AGENT`` picks
the one that runs.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Mapping

#: The coding agent to delegate to. Required. One of ``claude``, ``omp``,
#: ``cursor``, or ``agy``.
AGENT_ENV = "MOOSHIK_CODER_AGENT"
#: The four agent values this server knows how to spawn.
VALID_AGENTS = frozenset({"claude", "omp", "cursor", "agy"})

#: Anthropic API key — used when agent is ``claude``.
ANTHROPIC_API_KEY_ENV = "ANTHROPIC_API_KEY"
#: Gemini Developer API key — used when agent is ``omp``.
GEMINI_API_KEY_ENV = "MOOSHIK_GEMINI_API_KEY"
#: Vertex AI project id — used when agent is ``omp``.
GEMINI_PROJECT_ENV = "MOOSHIK_GEMINI_PROJECT"
#: Cursor Agent API key — used when agent is ``cursor``.
CURSOR_API_KEY_ENV = "CURSOR_API_KEY"

LOG_LEVEL_ENV = "CODER_LOG_LEVEL"
TIMEOUT_ENV = "CODER_TIMEOUT_SECS"

#: These tools return immediately (fire-and-forget + poll), so a short timeout
#: is appropriate. The 60s MCP_CALL_WAIT in the host is the hard upper bound;
#: this is deliberately much shorter so the server answers with a contained
#: timeout message rather than letting the host's firebreak fire.
DEFAULT_TIMEOUT_SECS = 10.0
DEFAULT_LOG_LEVEL = "INFO"


class ConfigError(Exception):
    """Configuration is missing or unusable. The message names variables only."""


@dataclass(frozen=True)
class Settings:
    """One resolved server process."""

    agent: str = "claude"
    anthropic_api_key: str | None = None
    gemini_api_key: str | None = None
    gemini_project: str | None = None
    cursor_api_key: str | None = None
    timeout_secs: float = DEFAULT_TIMEOUT_SECS
    log_level: str = DEFAULT_LOG_LEVEL

    def describe(self) -> str:
        """A one-line summary safe to log: names and modes, never values."""
        has_anthropic = "set" if self.anthropic_api_key else "unset"
        has_gemini_key = "set" if self.gemini_api_key else "unset"
        has_gemini_project = "set" if self.gemini_project else "unset"
        has_cursor = "set" if self.cursor_api_key else "unset"
        return (
            f"agent={self.agent} "
            f"anthropic_api_key={has_anthropic} "
            f"gemini_api_key={has_gemini_key} "
            f"gemini_project={has_gemini_project} "
            f"cursor_api_key={has_cursor} "
            f"timeout={self.timeout_secs}s"
        )

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        """Resolve settings from the process environment, failing closed."""
        env = os.environ if env is None else env

        agent = _clean(env.get(AGENT_ENV))
        if agent is None:
            raise ConfigError(
                f"no coding agent configured: set {AGENT_ENV} to one of "
                f"{', '.join(sorted(VALID_AGENTS))}. In config.toml this is "
                "the env.MOOSHIK_CODER_AGENT value under [mcp_servers.coder]."
            )
        if agent not in VALID_AGENTS:
            raise ConfigError(
                f"{AGENT_ENV}={agent!r} is not a supported agent. "
                f"Use one of: {', '.join(sorted(VALID_AGENTS))}."
            )

        return cls(
            agent=agent,
            anthropic_api_key=_clean(env.get(ANTHROPIC_API_KEY_ENV)),
            gemini_api_key=_clean(env.get(GEMINI_API_KEY_ENV)),
            gemini_project=_clean(env.get(GEMINI_PROJECT_ENV)),
            cursor_api_key=_clean(env.get(CURSOR_API_KEY_ENV)),
            timeout_secs=_positive_float(env, TIMEOUT_ENV, DEFAULT_TIMEOUT_SECS),
            log_level=(_clean(env.get(LOG_LEVEL_ENV)) or DEFAULT_LOG_LEVEL).upper(),
        )


def _clean(raw: str | None) -> str | None:
    """Treat an empty or whitespace-only variable as unset."""
    if raw is None:
        return None
    stripped = raw.strip()
    return stripped or None


def _positive_float(env: Mapping[str, str], name: str, default: float) -> float:
    raw = _clean(env.get(name))
    if raw is None:
        return default
    try:
        value = float(raw)
    except ValueError:
        raise ConfigError(f"{name} is not a number") from None
    if value <= 0:
        raise ConfigError(f"{name} must be greater than zero")
    return value
