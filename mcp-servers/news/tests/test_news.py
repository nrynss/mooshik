"""Offline suite: seams only — config, rendering, prompts, containment, and a
real stdio round trip against a faked backend. No network, no credentials,
nowhere."""

import asyncio
import os
import subprocess
import sys
import threading
from pathlib import Path

import pytest
from fakes import FakeClient, ScriptedBackend, grounded, raising_response, url_grounded
from news_mcp import backend as backend_mod
from news_mcp.backend import (
    GroundedBackend,
    NewsToolError,
    clamp_recency,
    fetch_prompt,
    normalise_url,
    redact,
    search_prompt,
)
from news_mcp.config import (
    API_KEY_ENV,
    CREDENTIALS_ENV,
    DEFAULT_LOCATION,
    DEFAULT_MODEL,
    LOCATION_ENV,
    MAX_CHARS_ENV,
    MODEL_ENV,
    PROJECT_ENV,
    TIMEOUT_ENV,
    ConfigError,
    Settings,
)
from news_mcp.render import (
    EMPTY_RESULT,
    TRUNCATION_NOTE,
    answer_text,
    clamp,
    collect_sources,
    render,
)
from news_mcp.tools import TIMEOUT_TEXT, UPSTREAM_TEXT, build_server, guarded

ROOT = Path(__file__).resolve().parents[1]


# ----------------------------------------------------------------- config ----


def test_no_credentials_at_all_fails_closed_naming_both_variables():
    with pytest.raises(ConfigError) as caught:
        Settings.from_env({})
    message = str(caught.value)
    assert API_KEY_ENV in message and PROJECT_ENV in message


def test_empty_and_whitespace_variables_count_as_unset():
    with pytest.raises(ConfigError):
        Settings.from_env({PROJECT_ENV: "   ", API_KEY_ENV: ""})


def test_project_alone_is_vertex_mode_with_defaults():
    settings = Settings.from_env({PROJECT_ENV: "mooshik-dev"})
    assert settings.use_vertex
    assert settings.project == "mooshik-dev"
    assert settings.location == DEFAULT_LOCATION
    assert settings.model == DEFAULT_MODEL
    assert settings.credentials_path is None


def test_api_key_alone_is_developer_api_mode():
    settings = Settings.from_env({API_KEY_ENV: "AIza-not-a-real-key"})
    assert not settings.use_vertex
    assert settings.project is None


def test_every_knob_comes_from_the_environment():
    settings = Settings.from_env(
        {
            PROJECT_ENV: "p",
            LOCATION_ENV: "us-central1",
            CREDENTIALS_ENV: "/tmp/sa.json",
            MODEL_ENV: "gemini-2.5-pro",
            TIMEOUT_ENV: "12.5",
            MAX_CHARS_ENV: "900",
        }
    )
    assert (settings.location, settings.model) == ("us-central1", "gemini-2.5-pro")
    assert (settings.timeout_secs, settings.max_chars) == (12.5, 900)
    assert settings.credentials_path == "/tmp/sa.json"


@pytest.mark.parametrize(
    ("name", "value"),
    [(TIMEOUT_ENV, "soon"), (TIMEOUT_ENV, "0"), (MAX_CHARS_ENV, "lots"), (MAX_CHARS_ENV, "-1")],
)
def test_unusable_numeric_knobs_fail_closed_naming_the_variable(name, value):
    with pytest.raises(ConfigError) as caught:
        Settings.from_env({PROJECT_ENV: "p", name: value})
    assert name in str(caught.value)


def test_describe_never_echoes_the_api_key():
    key = "AIza-SUPER-SECRET-VALUE"
    described = Settings.from_env({API_KEY_ENV: key}).describe()
    assert key not in described
    assert "api-key" in described


# ----------------------------------------------------------------- render ----


def test_sources_are_extracted_deduped_and_order_preserved():
    response = grounded(
        "Two outlets covered it.",
        sources=[
            ("https://a.example/1", "Story one", "a.example"),
            ("https://b.example/2", "Story two", "b.example"),
            ("https://a.example/1", "Story one again", "a.example"),
        ],
    )
    assert [s.uri for s in collect_sources(response)] == [
        "https://a.example/1",
        "https://b.example/2",
    ]


def test_url_context_metadata_is_a_source_too():
    response = url_grounded("The page says X.", urls=["https://c.example/post"])
    assert [s.uri for s in collect_sources(response)] == ["https://c.example/post"]


def test_render_puts_the_answer_first_and_sources_last():
    response = grounded(
        "Ship it.", sources=[("https://a.example/1", "Story one", "a.example")]
    )
    out = render(response, max_chars=1000)
    assert out.startswith("Ship it.")
    assert "## Sources" in out
    assert "[Story one](https://a.example/1)" in out


def test_an_answer_with_no_sources_is_marked_unverified():
    out = render(grounded("Trust me.", sources=[]), max_chars=1000)
    assert "unverified" in out


def test_a_wholly_empty_response_is_a_contained_no_result():
    assert render(grounded("", sources=[]), max_chars=1000) == EMPTY_RESULT


def test_a_raising_text_property_does_not_crash_the_renderer():
    response = raising_response()
    assert answer_text(response) == ""
    assert render(response, max_chars=100) == EMPTY_RESULT


def test_clamped_answers_keep_their_sources():
    response = grounded(
        "word " * 400, sources=[("https://a.example/1", "Story one", "a.example")]
    )
    out = render(response, max_chars=200)
    assert TRUNCATION_NOTE.strip() in out
    assert "https://a.example/1" in out  # provenance survives truncation


def test_clamp_is_a_noop_under_budget_and_breaks_on_whitespace():
    assert clamp("short", 100) == "short"
    clamped = clamp("alpha beta gamma delta epsilon", 20)
    body = clamped[: -len(TRUNCATION_NOTE)]
    assert clamped.endswith(TRUNCATION_NOTE)
    assert len(body) <= 20
    assert "alpha beta gamma" in body  # broke on a space, not mid-word
    assert not body.endswith("del")


def test_search_queries_are_shown_only_when_asked():
    response = grounded("x", sources=[("https://a/1", "A", "a")], queries=["tech news"])
    assert "Searched:" in render(response, max_chars=500, show_queries=True)
    assert "Searched:" not in render(response, max_chars=500, show_queries=False)


# ---------------------------------------------------------------- backend ----


def _backend(responses, **kw):
    kw.setdefault("clock", lambda: "2026-08-27")
    return GroundedBackend(FakeClient(responses), max_chars=4000, **kw)


def test_search_grounds_with_google_search_and_carries_the_query():
    backend = _backend([grounded("Answer.", sources=[("https://a/1", "A", "a")])])
    out = backend.search("what happened in tech today", recency_days=3)

    call = backend.client.calls[0]
    assert "what happened in tech today" in call["contents"]
    assert "3 day(s)" in call["contents"]
    assert "2026-08-27" in call["contents"]
    assert call["config"].tools[0].google_search is not None
    assert call["config"].tools[0].url_context is None
    assert "Answer." in out and "https://a/1" in out


def test_fetch_grounds_with_url_context_and_carries_the_focus():
    backend = _backend([url_grounded("Page says X.", urls=["https://c/post"])])
    out = backend.fetch("https://c/post", focus="the benchmark numbers")

    call = backend.client.calls[0]
    assert "https://c/post" in call["contents"]
    assert "the benchmark numbers" in call["contents"]
    assert call["config"].tools[0].url_context is not None
    assert call["config"].tools[0].google_search is None
    assert "https://c/post" in out


def test_the_sdk_call_carries_the_configured_timeout():
    backend = _backend([grounded("x")], timeout_secs=9.0)
    backend.search("q")
    assert backend.client.calls[0]["config"].http_options.timeout == 9000


@pytest.mark.parametrize("bad", ["", "   ", None, 17])
def test_an_empty_query_is_a_message_not_a_call(bad):
    backend = _backend([])
    with pytest.raises(NewsToolError):
        backend.search(bad)
    assert backend.client.calls == []


@pytest.mark.parametrize(
    "bad", ["file:///etc/passwd", "data:text/html,x", "ftp://host/f", "example.com"]
)
def test_non_http_urls_are_refused_before_any_call(bad):
    backend = _backend([])
    with pytest.raises(NewsToolError):
        backend.fetch(bad)
    assert backend.client.calls == []


def test_a_plain_https_url_survives_normalisation():
    assert normalise_url("  https://a.example/x  ") == "https://a.example/x"


@pytest.mark.parametrize(
    ("given", "expected"), [(0, 1), (1, 1), (7, 7), (10_000, 365), ("nope", 7), (None, 7)]
)
def test_recency_is_clamped_into_a_supported_window(given, expected):
    assert clamp_recency(given) == expected


def test_prompts_are_pure_functions_of_their_arguments():
    assert "QUESTION:\nwhat is up" in search_prompt("what is up", 7, "2026-08-27")
    assert "Focus on this" not in fetch_prompt("https://a/1", "")
    assert "Focus on this in particular: tables" in fetch_prompt("https://a/1", "tables")


def test_a_secret_echoed_by_upstream_is_scrubbed_from_the_result():
    key = "AIza-SUPER-SECRET-VALUE"
    backend = _backend(
        [grounded(f"The upstream said your key {key} is bad.")], secrets=(key,)
    )
    out = backend.search("q")
    assert key not in out
    assert "[redacted]" in out


def test_redact_ignores_empty_secrets():
    assert redact("nothing to see", ("", None)) == "nothing to see"


def test_make_client_is_only_imported_when_actually_building_one():
    # The offline suite never constructs a real client; assert the seam exists
    # and that importing the module did not drag in google.auth.
    assert callable(backend_mod.make_client)
    assert "google.oauth2.service_account" not in sys.modules


# ------------------------------------------------------- tool containment ----


def _guard(call, **kw):
    kw.setdefault("timeout_secs", 5.0)
    return asyncio.run(guarded("t", call, **kw))


def test_an_upstream_exception_becomes_a_readable_tool_result():
    def boom():
        raise RuntimeError("connection reset by peer")

    out = _guard(boom)
    assert out == UPSTREAM_TEXT.format(kind="RuntimeError")
    assert "connection reset" not in out  # detail stays on stderr


def test_a_slow_backend_answers_with_a_timeout_not_a_hang():
    # The abandoned worker thread is released in `finally` only so the test
    # exits promptly; the point is that `guarded` already answered without it.
    release = threading.Event()

    async def go():
        try:
            return await guarded(
                "t", lambda: release.wait(30) and "late", timeout_secs=0.05
            )
        finally:
            release.set()

    assert asyncio.run(go()) == TIMEOUT_TEXT.format(secs=0.05)


def test_an_expected_error_keeps_its_own_message():
    def refused():
        raise NewsToolError("Unsupported URL scheme in 'file'.")

    assert "Unsupported URL scheme" in _guard(refused)


def test_secrets_never_ride_out_on_an_expected_error():
    key = "AIza-SUPER-SECRET-VALUE"

    def refused():
        raise NewsToolError(f"rejected key {key}")

    out = _guard(refused, secrets=(key,))
    assert key not in out and "[redacted]" in out


# ------------------------------------------------------------ the surface ----


def test_the_exposed_surface_is_exactly_two_named_tools():
    server = build_server(ScriptedBackend())
    tools = asyncio.run(server.list_tools())
    assert sorted(t.name for t in tools) == ["fetch_article", "search_news"]


# ------------------------------------------------------------- stdio wire ----


def _child_env(**extra):
    """The child needs a working interpreter and nothing else."""
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": os.environ.get("HOME", ""),
        "PYTHONPATH": str(ROOT),
    }
    env.update(extra)
    return env


async def _round_trip(mode: str, call=None, timeout_secs: str = "10"):
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    params = StdioServerParameters(
        command=sys.executable,
        args=[str(ROOT / "tests" / "wire_server.py")],
        env=_child_env(WIRE_MODE=mode, WIRE_TIMEOUT_SECS=timeout_secs),
    )
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            init = await session.initialize()
            listed = await session.list_tools()
            result = None
            if call is not None:
                result = await session.call_tool(*call)
            return init, listed, result


@pytest.mark.parametrize("mode", ["ok"])
def test_initialize_and_tools_list_over_a_real_stdio_transport(mode):
    init, listed, _ = asyncio.run(_round_trip(mode))

    assert init.server_info.name == "mooshik-news"
    assert "grounded" in (init.instructions or "")

    tools = {tool.name: tool for tool in listed.tools}
    assert sorted(tools) == ["fetch_article", "search_news"]

    search = tools["search_news"]
    assert "Sources list" in (search.description or "")
    properties = search.input_schema["properties"]
    assert set(properties) == {"query", "recency_days"}
    assert search.input_schema["required"] == ["query"]
    assert "plain language" in properties["query"]["description"]

    fetch = tools["fetch_article"]
    assert set(fetch.input_schema["properties"]) == {"url", "focus"}
    assert fetch.input_schema["required"] == ["url"]


def test_a_tool_call_returns_the_backend_text_over_the_wire():
    _, _, result = asyncio.run(
        _round_trip("ok", ("search_news", {"query": "tech news", "recency_days": 2}))
    )
    text = result.content[0].text
    assert "canned answer" in text
    assert "query=tech news recency_days=2" in text
    assert not result.is_error


def test_an_upstream_failure_arrives_as_a_result_not_a_dead_child():
    _, _, result = asyncio.run(
        _round_trip("boom", ("search_news", {"query": "anything"}))
    )
    assert result.content[0].text == UPSTREAM_TEXT.format(kind="RuntimeError")
    assert not result.is_error


def test_a_hung_backend_still_answers_within_the_bound():
    _, _, result = asyncio.run(
        _round_trip("hang", ("search_news", {"query": "anything"}), timeout_secs="0.2")
    )
    assert "Timed out" in result.content[0].text


def test_a_stray_print_in_a_tool_body_does_not_corrupt_the_framing():
    _, _, result = asyncio.run(
        _round_trip("noisy", ("search_news", {"query": "tech news"}))
    )
    assert "canned answer" in result.content[0].text
    assert "STRAY STDOUT" not in result.content[0].text


# --------------------------------------------------------- the entrypoint ----


def _run_entrypoint(env, args=()):
    return subprocess.run(
        [sys.executable, str(ROOT / "server.py"), *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=60,
    )


def test_missing_credentials_exit_nonzero_with_nothing_on_stdout():
    done = _run_entrypoint(_child_env())
    assert done.returncode == 2
    assert done.stdout == ""  # stdout is the JSON-RPC channel; never touch it
    assert PROJECT_ENV in done.stderr and API_KEY_ENV in done.stderr


def test_secrets_are_never_accepted_as_command_line_arguments():
    done = _run_entrypoint(_child_env(**{PROJECT_ENV: "p"}), args=["--api-key", "x"])
    assert done.returncode == 2
    assert done.stdout == ""
    assert "environment" in done.stderr
