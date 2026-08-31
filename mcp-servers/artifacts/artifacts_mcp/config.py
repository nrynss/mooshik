from __future__ import annotations
import os
from dataclasses import dataclass
from typing import Mapping

from mooshik_common.models import DEFAULT_LOCATION as COMMON_LOCATION
from mooshik_common.models import DEFAULT_MODEL as COMMON_MODEL

API_KEY_ENV = "MOOSHIK_GEMINI_API_KEY"
PROJECT_ENV = "MOOSHIK_GEMINI_PROJECT"
LOCATION_ENV = "ARTIFACTS_LOCATION"
CREDENTIALS_ENV = "MOOSHIK_GEMINI_CREDENTIALS"

MODEL_ENV = "ARTIFACTS_MODEL"
TIMEOUT_ENV = "ARTIFACTS_TIMEOUT_SECS"
LOG_LEVEL_ENV = "ARTIFACTS_LOG_LEVEL"

DEFAULT_MODEL = COMMON_MODEL
DEFAULT_LOCATION = COMMON_LOCATION
DEFAULT_TIMEOUT_SECS = 45.0
DEFAULT_LOG_LEVEL = "INFO"

class ConfigError(Exception): pass

@dataclass(frozen=True)
class Settings:
    model: str = DEFAULT_MODEL
    api_key: str | None = None
    project: str | None = None
    location: str = DEFAULT_LOCATION
    credentials_path: str | None = None
    timeout_secs: float = DEFAULT_TIMEOUT_SECS
    log_level: str = DEFAULT_LOG_LEVEL

    @property
    def use_vertex(self) -> bool:
        return self.api_key is None

    def describe(self) -> str:
        auth = "api-key" if self.api_key else "vertex"
        where = f"project={self.project} location={self.location}" if self.use_vertex else "developer-api"
        creds = "service-account-file" if self.credentials_path else "default"
        return f"model={self.model} auth={auth} {where} credentials={creds} timeout={self.timeout_secs}s"

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        env = os.environ if env is None else env
        api_key = _clean(env.get(API_KEY_ENV))
        project = _clean(env.get(PROJECT_ENV))
        if api_key is None and project is None:
            raise ConfigError(f"no Google credentials in the environment: set {API_KEY_ENV} or {PROJECT_ENV}")
        return cls(
            model=_clean(env.get(MODEL_ENV)) or DEFAULT_MODEL,
            api_key=api_key,
            project=project,
            location=_clean(env.get(LOCATION_ENV)) or DEFAULT_LOCATION,
            credentials_path=_clean(env.get(CREDENTIALS_ENV)),
            timeout_secs=_positive_float(env, TIMEOUT_ENV, DEFAULT_TIMEOUT_SECS),
            log_level=(_clean(env.get(LOG_LEVEL_ENV)) or DEFAULT_LOG_LEVEL).upper(),
        )

def _clean(raw: str | None) -> str | None:
    if raw is None: return None
    stripped = raw.strip()
    return stripped or None

def _positive_float(env: Mapping[str, str], name: str, default: float) -> float:
    raw = _clean(env.get(name))
    if raw is None: return default
    try: value = float(raw)
    except ValueError: raise ConfigError(f"{name} is not a number") from None
    if value <= 0: raise ConfigError(f"{name} must be greater than zero")
    return value
