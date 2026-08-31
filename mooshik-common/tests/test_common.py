"""Offline suite: no network, no credentials, no real SDK client."""

import subprocess
import sys
import types

import pytest
from mooshik_common.concepts import (
    CONCEPT_TYPES,
    MAX_CONCEPT_CHARS,
    MAX_CONCEPTS_PER_CHUNK,
    Concept,
    parse_concepts,
)
from mooshik_common.models import (
    DEFAULT_LOCATION,
    DEFAULT_MODEL,
    EMBEDDER_LOCATION_ENV,
    GLOBAL_LOCATION,
)
from mooshik_common.vertex import (
    CLOUD_PLATFORM_SCOPE,
    credentials_description,
    make_client,
)


# ------------------------------------------------------------------ models --

def test_the_default_model_is_at_or_above_the_supported_floor():
    major, minor = DEFAULT_MODEL.removeprefix("gemini-").split("-")[0].split(".")
    assert (int(major), int(minor)) >= (3, 5), DEFAULT_MODEL


def test_inference_defaults_to_global_and_never_to_the_embedder_variable():
    """The pair of defects this package exists to stop recurring."""
    assert DEFAULT_LOCATION == GLOBAL_LOCATION == "global"
    assert EMBEDDER_LOCATION_ENV == "MOOSHIK_GEMINI_LOCATION"
    assert DEFAULT_LOCATION != EMBEDDER_LOCATION_ENV


# ---------------------------------------------------------------- concepts --

def test_the_vocabulary_is_the_five_lambo_accepts():
    assert CONCEPT_TYPES == {
        "entity", "logic", "constraint", "resource", "observation",
    }


def test_parse_keeps_only_valid_typed_concepts():
    got = parse_concepts(
        '[{"content":"a","concept_type":"entity"},'
        ' {"content":"b","concept_type":"nonsense"},'
        ' {"content":"  ","concept_type":"logic"},'
        ' "not an object"]'
    )
    assert got == [Concept(content="a", concept_type="entity")]


def test_parse_strips_code_fences_and_surrounding_prose():
    got = parse_concepts(
        'Sure! Here you go:\n```json\n[{"content":"x","concept_type":"logic"}]\n```\nHope that helps.'
    )
    assert got == [Concept(content="x", concept_type="logic")]


def test_content_is_clamped_and_the_batch_is_capped():
    long_one = parse_concepts(
        '[{"content":"%s","concept_type":"entity"}]' % ("z" * (MAX_CONCEPT_CHARS + 500))
    )
    assert len(long_one[0].content) == MAX_CONCEPT_CHARS

    many = ",".join(['{"content":"c%d","concept_type":"entity"}' % i for i in range(MAX_CONCEPTS_PER_CHUNK + 10)])
    assert len(parse_concepts("[" + many + "]")) == MAX_CONCEPTS_PER_CHUNK


@pytest.mark.parametrize("raw", ["no array here", "{}", "]["])
def test_unparseable_output_raises_rather_than_returning_nothing(raw):
    """A caller must be able to tell 'the model said nothing qualifies' (`[]`)
    from 'the model did not answer in the contract'. Returning [] for both
    would silently swallow a broken extraction."""
    with pytest.raises(ValueError):
        parse_concepts(raw)


def test_an_empty_array_is_a_valid_answer():
    assert parse_concepts("[]") == []


# ------------------------------------------------------------------ vertex --

class FakeGenai(types.ModuleType):
    def __init__(self):
        super().__init__("google.genai")
        self.seen = None

        outer = self

        class Client:
            def __init__(self, **kwargs):
                outer.seen = kwargs

        self.Client = Client


@pytest.fixture
def fake_genai(monkeypatch):
    fake = FakeGenai()
    google = types.ModuleType("google")
    google.genai = fake
    monkeypatch.setitem(sys.modules, "google", google)
    monkeypatch.setitem(sys.modules, "google.genai", fake)
    return fake


def test_an_api_key_selects_developer_api_mode_with_no_project(fake_genai):
    make_client(api_key="k", project="ignored", location="ignored")
    assert fake_genai.seen == {"api_key": "k"}


def test_vertex_mode_passes_project_and_location(fake_genai):
    make_client(project="p", location="global")
    assert fake_genai.seen == {"vertexai": True, "project": "p", "location": "global"}


def test_absent_arguments_are_omitted_rather_than_sent_as_none(fake_genai):
    make_client(project="p")
    assert "location" not in fake_genai.seen
    assert "credentials" not in fake_genai.seen


def test_the_genai_import_is_lazy():
    """news_mcp's offline suite depends on this: importing the module must not
    drag in the SDK or the auth libraries. A module-scope import here would
    make every network-free test run require both."""
    probe = "import sys, mooshik_common.vertex; print('google.genai' in sys.modules)"
    out = subprocess.run([sys.executable, "-c", probe], capture_output=True, text=True)
    assert out.stdout.strip() == "False", out.stdout + out.stderr


class FakeOauth2(types.ModuleType):
    """Records which credential constructor `make_client` chooses."""

    def __init__(self):
        super().__init__("google.oauth2")
        self.calls = []
        outer = self

        class UserCredentials:
            @classmethod
            def from_authorized_user_file(cls, path, **kwargs):
                outer.calls.append(("authorized_user", path, kwargs))
                return "authorized-user-credentials"

        class ServiceAccountCredentials:
            @classmethod
            def from_service_account_file(cls, path, **kwargs):
                outer.calls.append(("service_account", path, kwargs))
                return "service-account-credentials"

        credentials = types.ModuleType("google.oauth2.credentials")
        credentials.Credentials = UserCredentials
        service_account = types.ModuleType("google.oauth2.service_account")
        service_account.Credentials = ServiceAccountCredentials
        self.credentials = credentials
        self.service_account = service_account


@pytest.fixture
def fake_google(monkeypatch):
    fake = FakeGenai()
    oauth2 = FakeOauth2()
    google = types.ModuleType("google")
    google.genai = fake
    google.oauth2 = oauth2
    monkeypatch.setitem(sys.modules, "google", google)
    monkeypatch.setitem(sys.modules, "google.genai", fake)
    monkeypatch.setitem(sys.modules, "google.oauth2", oauth2)
    return fake, oauth2


SERVICE_ACCOUNT_JSON = (
    '{"type": "service_account", "project_id": "p", '
    '"client_email": "a@b", "private_key": "x"}'
)
AUTHORIZED_USER_JSON = (
    '{"type": "authorized_user", "client_id": "c", '
    '"client_secret": "s", "refresh_token": "t"}'
)


def test_a_service_account_file_builds_a_client_unchanged(fake_google, tmp_path):
    fake, oauth2 = fake_google
    path = tmp_path / "sa.json"
    path.write_text(SERVICE_ACCOUNT_JSON)
    make_client(project="p", location="global", credentials_path=str(path))
    assert oauth2.calls == [
        ("service_account", str(path), {"scopes": [CLOUD_PLATFORM_SCOPE]})
    ]
    assert fake.seen["credentials"] == "service-account-credentials"


def test_an_authorized_user_file_builds_a_client(fake_google, tmp_path):
    """The regression: a gcloud application-default file is `authorized_user`,
    and the old loader rejected it with MalformedError."""
    fake, oauth2 = fake_google
    path = tmp_path / "adc.json"
    path.write_text(AUTHORIZED_USER_JSON)
    make_client(project="p", location="global", credentials_path=str(path))
    assert oauth2.calls == [
        ("authorized_user", str(path), {"scopes": [CLOUD_PLATFORM_SCOPE]})
    ]
    assert fake.seen["credentials"] == "authorized-user-credentials"


def test_a_missing_credentials_file_fails_naming_the_path(fake_google, tmp_path):
    with pytest.raises(ValueError) as exc:
        make_client(project="p", credentials_path=str(tmp_path / "nope.json"))
    assert "nope.json" in str(exc.value)
    assert "does not exist" in str(exc.value)


def test_a_malformed_credentials_file_fails_naming_the_path(fake_google, tmp_path):
    path = tmp_path / "bad.json"
    path.write_text("not json at all")
    with pytest.raises(ValueError) as exc:
        make_client(project="p", credentials_path=str(path))
    assert str(path) in str(exc.value)


def test_an_unknown_credential_type_fails_naming_the_path(fake_google, tmp_path):
    path = tmp_path / "other.json"
    path.write_text('{"type": "external_account"}')
    with pytest.raises(ValueError) as exc:
        make_client(project="p", credentials_path=str(path))
    assert str(path) in str(exc.value)


def test_credentials_description_says_what_the_file_is(tmp_path):
    assert credentials_description(None) == "default"
    service = tmp_path / "sa.json"
    service.write_text(SERVICE_ACCOUNT_JSON)
    assert credentials_description(str(service)) == "service-account-file"
    adc = tmp_path / "adc.json"
    adc.write_text(AUTHORIZED_USER_JSON)
    assert credentials_description(str(adc)) == "authorized-user-file"
    assert credentials_description(str(tmp_path / "missing.json")) == "unknown"


def test_the_scope_is_cloud_platform():
    assert CLOUD_PLATFORM_SCOPE == "https://www.googleapis.com/auth/cloud-platform"
