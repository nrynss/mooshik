"""One `google-genai` client construction, for every component.

**Every import of `google.genai` in here is lazy, and must stay that way.**
The offline suites pass fakes straight into their backends and never touch
credentials or auth libraries; an import at module scope would drag both into
every test run and make a network-free suite depend on an SDK being installed.
"""

from __future__ import annotations

import json
from typing import Any

#: The scope a service-account credential is minted with. Both the Vertex
#: inference path and the Cloud SQL proxy accept it.
CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"


def _file_credential_kind(path: str) -> tuple[str | None, str | None]:
    """The `type` field of a credentials file, or the problem with the file.

    Returns `(kind, None)` when the file reads as JSON and carries a `type`
    field, and `(None, problem)` otherwise, where `problem` is a full
    sentence naming the file. `gcloud auth application-default login` writes
    `authorized_user`; a service account writes `service_account`.
    """
    try:
        with open(path, encoding="utf-8") as handle:
            kind = json.load(handle).get("type")
    except FileNotFoundError:
        return None, f"credentials file {path} does not exist"
    except OSError as error:
        return None, f"could not read credentials file {path}: {error}"
    except json.JSONDecodeError as error:
        return None, f"credentials file {path} is not valid JSON: {error}"
    if kind is None:
        return None, f"credentials file {path} has no type field"
    return kind, None


def _credentials_from_file(path: str) -> Any:
    """Build credentials from a file, whichever Google type it is.

    A service-account file and a gcloud application-default (`authorized_user`)
    file both work. Anything else fails with a message naming the file.
    """
    from google.oauth2 import credentials as user_credentials
    from google.oauth2 import service_account

    kind, problem = _file_credential_kind(path)
    if problem is not None:
        raise ValueError(problem)
    if kind == "service_account":
        return service_account.Credentials.from_service_account_file(
            path,
            scopes=[CLOUD_PLATFORM_SCOPE],
        )
    if kind == "authorized_user":
        return user_credentials.Credentials.from_authorized_user_file(
            path,
            scopes=[CLOUD_PLATFORM_SCOPE],
        )
    raise ValueError(
        f"credentials file {path} has type {kind!r}; "
        "expected service_account or authorized_user"
    )


def credentials_description(credentials_path: str | None) -> str:
    """One phrase for what a credentials file is, for a startup log line.

    Never raises: this is description only, and the loader's error is the
    failure surface. A file that cannot be read reports itself as unknown.
    """
    if not credentials_path:
        return "default"
    kind, _ = _file_credential_kind(credentials_path)
    if kind == "service_account":
        return "service-account-file"
    if kind == "authorized_user":
        return "authorized-user-file"
    return "unknown"

def make_client(
    *,
    project: str | None = None,
    location: str | None = None,
    credentials_path: str | None = None,
    api_key: str | None = None,
) -> Any:
    """Build a `genai.Client` in whichever mode the arguments describe.

    `api_key` selects the Gemini Developer API and needs neither project nor
    location. Otherwise the client is Vertex-mode; `credentials_path` names a
    service-account json, and without one the SDK falls back to application
    default credentials.

    Keyword-only on purpose: the two call sites this replaced took their
    arguments in different orders, and a positional signature would have made
    that a silent swap rather than a `TypeError`.
    """
    from google import genai

    if api_key:
        return genai.Client(api_key=api_key)

    kwargs: dict[str, Any] = {"vertexai": True}
    if project:
        kwargs["project"] = project
    if location:
        kwargs["location"] = location
    if credentials_path:
        kwargs["credentials"] = _credentials_from_file(credentials_path)
    return genai.Client(**kwargs)
