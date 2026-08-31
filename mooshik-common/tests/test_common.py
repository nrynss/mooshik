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
from mooshik_common.vertex import CLOUD_PLATFORM_SCOPE, make_client


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


def test_the_scope_is_cloud_platform():
    assert CLOUD_PLATFORM_SCOPE == "https://www.googleapis.com/auth/cloud-platform"
