"""Environment-driven settings for the news MCP server.

Mooshik spawns this server as a child process over stdio and injects the
credentials it needs as **environment variables**, resolved from its encrypted
vault at spawn time (`src/mcp_host/mod.rs`: "the `env` map on each config entry
names vault secrets — the operator writes *names*, never values"). Everything
here follows from that contract:

* All configuration comes from the environment. No config file is read, and no
  secret is ever accepted as a CLI argument, where it would land in `ps` output
  and shell history.
* A missing variable fails closed, naming the variable. Nothing about a
  *value* is ever logged or put in an error message — only whether a name was
  set, so a stderr transcript is safe to paste into a bug report.

The credential names are deliberately the same `MOOSHIK_GEMINI_*` names the
bootstrap ingester reads, so one vault secret serves both.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Mapping

#: Gemini Developer API key. When set, the client runs in API-key mode and no
#: project/location is needed. Secret: never logged, never echoed.
API_KEY_ENV = "MOOSHIK_GEMINI_API_KEY"
#: Vertex AI project id. Required unless `MOOSHIK_GEMINI_API_KEY` is set.
PROJECT_ENV = "MOOSHIK_GEMINI_PROJECT"
#: Vertex AI location; defaults to `global`, which is where Search grounding
#: is served.
LOCATION_ENV = "MOOSHIK_GEMINI_LOCATION"
#: Path to a service-account JSON file. Optional: without it the client falls
#: back to application-default credentials.
CREDENTIALS_ENV = "MOOSHIK_GEMINI_CREDENTIALS"

MODEL_ENV = "NEWS_MODEL"
TIMEOUT_ENV = "NEWS_TIMEOUT_SECS"
MAX_CHARS_ENV = "NEWS_MAX_CHARS"
LOG_LEVEL_ENV = "NEWS_LOG_LEVEL"

DEFAULT_MODEL = "gemini-2.5-flash"
DEFAULT_LOCATION = "global"
#: Per-call wall clock. Mooshik applies its own 60s bound per MCP call; this
#: one is deliberately shorter so the server answers with a contained timeout
#: message rather than letting the host's firebreak fire and show the model a
#: bare internal error.
DEFAULT_TIMEOUT_SECS = 45.0
#: Results land in the companion's context window and may be written into the
#: user's memory. A runaway answer would evict the conversation, so clamp.
DEFAULT_MAX_CHARS = 6_000
DEFAULT_LOG_LEVEL = "INFO"


class ConfigError(Exception):
    """Configuration is missing or unusable. The message names variables only."""


@dataclass(frozen=True)
class Settings:
    """One resolved server process."""

    model: str = DEFAULT_MODEL
    api_key: str | None = None
    project: str | None = None
    location: str = DEFAULT_LOCATION
    credentials_path: str | None = None
    timeout_secs: float = DEFAULT_TIMEOUT_SECS
    max_chars: int = DEFAULT_MAX_CHARS
    log_level: str = DEFAULT_LOG_LEVEL

    @property
    def use_vertex(self) -> bool:
        """Vertex mode unless an API key was supplied."""
        return self.api_key is None

    def describe(self) -> str:
        """A one-line summary safe to log: names and modes, never values."""
        auth = "api-key" if self.api_key else "vertex"
        where = (
            f"project={self.project} location={self.location}"
            if self.use_vertex
            else "developer-api"
        )
        creds = "service-account-file" if self.credentials_path else "default"
        return (
            f"model={self.model} auth={auth} {where} credentials={creds} "
            f"timeout={self.timeout_secs}s max_chars={self.max_chars}"
        )

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        """Resolve settings from the process environment, failing closed."""
        env = os.environ if env is None else env

        api_key = _clean(env.get(API_KEY_ENV))
        project = _clean(env.get(PROJECT_ENV))
        if api_key is None and project is None:
            raise ConfigError(
                f"no Google credentials in the environment: set {API_KEY_ENV} "
                f"(Gemini Developer API) or {PROJECT_ENV} (Vertex AI). In "
                "config.toml these are vault secret NAMES under "
                "[mcp_servers.news.env], never literal values."
            )

        return cls(
            model=_clean(env.get(MODEL_ENV)) or DEFAULT_MODEL,
            api_key=api_key,
            project=project,
            location=_clean(env.get(LOCATION_ENV)) or DEFAULT_LOCATION,
            credentials_path=_clean(env.get(CREDENTIALS_ENV)),
            timeout_secs=_positive_float(env, TIMEOUT_ENV, DEFAULT_TIMEOUT_SECS),
            max_chars=_positive_int(env, MAX_CHARS_ENV, DEFAULT_MAX_CHARS),
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


def _positive_int(env: Mapping[str, str], name: str, default: int) -> int:
    raw = _clean(env.get(name))
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError:
        raise ConfigError(f"{name} is not an integer") from None
    if value <= 0:
        raise ConfigError(f"{name} must be greater than zero")
    return value
