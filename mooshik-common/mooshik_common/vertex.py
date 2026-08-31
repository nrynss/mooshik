"""One `google-genai` client construction, for every component.

**Every import of `google.genai` in here is lazy, and must stay that way.**
The offline suites pass fakes straight into their backends and never touch
credentials or auth libraries; an import at module scope would drag both into
every test run and make a network-free suite depend on an SDK being installed.
"""

from __future__ import annotations

from typing import Any

#: The scope a service-account credential is minted with. Both the Vertex
#: inference path and the Cloud SQL proxy accept it.
CLOUD_PLATFORM_SCOPE = "https://www.googleapis.com/auth/cloud-platform"


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
        from google.oauth2 import service_account

        kwargs["credentials"] = service_account.Credentials.from_service_account_file(
            credentials_path,
            scopes=[CLOUD_PLATFORM_SCOPE],
        )
    return genai.Client(**kwargs)
