"""Offline tests for the coder MCP server.

15+ tests covering configuration, wire round-trips, backend logic,
standing rule writing, and the entrypoint. No network, no credentials,
no coding agent binary required.

The wire tests spawn ``wire_server.py`` as a child and exercise the full
JSON-RPC round trip — exactly as Mooshik's host would. The backend tests
use ``FakeProcess`` to avoid spawning real processes.
"""

from __future__ import annotations

import asyncio
import json
import os
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from coder_mcp.config import (
    Settings,
    ConfigError,
    AGENT_ENV,
    VALID_AGENTS,
    CURSOR_API_KEY_ENV,
)
import importlib.util

from coder_mcp.standing_rule import write_standing_rule, STANDING_RULE_TEMPLATE
from coder_mcp.backend import CoderBackend
from coder_mcp.errors import CoderToolError
from coder_mcp.agents import build_agent_command

ROOT = Path(__file__).resolve().parents[1]


def _load_fakes():
    fakes_path = Path(__file__).resolve().parent / "fakes.py"
    spec = importlib.util.spec_from_file_location("coder_test_fakes", fakes_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.FakeProcess, module.ScriptedBackend


FakeProcess, ScriptedBackend = _load_fakes()


# ── Configuration tests ──────────────────────────────────────────────────


def test_missing_agent_env_fails_closed():
    """ConfigError when MOOSHIK_CODER_AGENT is missing."""
    with pytest.raises(ConfigError) as caught:
        Settings.from_env({})
    assert AGENT_ENV in str(caught.value)


def test_invalid_agent_fails_closed():
    """ConfigError for an unknown agent name."""
    with pytest.raises(ConfigError) as caught:
        Settings.from_env({AGENT_ENV: "gpt-5"})
    assert "gpt-5" in str(caught.value)


def test_claude_agent_resolves():
    """Settings with agent=claude parses without error."""
    s = Settings.from_env({AGENT_ENV: "claude"})
    assert s.agent == "claude"


def test_omp_agent_resolves():
    """Settings with agent=omp parses without error."""
    s = Settings.from_env({AGENT_ENV: "omp"})
    assert s.agent == "omp"


def test_cursor_agent_resolves():
    """Settings with agent=cursor parses without error."""
    s = Settings.from_env({AGENT_ENV: "cursor"})
    assert s.agent == "cursor"


def test_agy_agent_resolves():
    """Settings with agent=agy parses without error."""
    s = Settings.from_env({AGENT_ENV: "agy"})
    assert s.agent == "agy"


def test_agy_agent_command_built():
    """agy agent builds agy -p <task> --dangerously-skip-permissions command."""
    cmd, args, env = build_agent_command(
        "agy",
        "implement feature",
        "/tmp/repo",
        "/tmp/repo/AGENTS.md",
    )
    assert cmd == "agy"
    assert "--dangerously-skip-permissions" in args
    assert "-p" in args
    assert "AGENTS.md" in args[args.index("-p") + 1]
    assert "implement feature" in args[args.index("-p") + 1]


def test_cursor_api_key_configured_and_forwarded():
    """CURSOR_API_KEY is parsed into Settings, included in describe(), and forwarded."""
    s = Settings.from_env({
        AGENT_ENV: "cursor",
        CURSOR_API_KEY_ENV: "cur-secret-key-12345",
    })
    assert s.agent == "cursor"
    assert s.cursor_api_key == "cur-secret-key-12345"
    desc = s.describe()
    assert "cur-secret-key-12345" not in desc
    assert "cursor_api_key=set" in desc

    # Forwarding into agent command environment
    cmd, args, env = build_agent_command(
        "cursor",
        "fix bug",
        "/tmp/repo",
        None,
        env_overrides={"CURSOR_API_KEY": s.cursor_api_key or ""},
    )
    assert env.get("CURSOR_API_KEY") == "cur-secret-key-12345"


def test_settings_describe_never_leaks_values():
    """The describe() output names modes but never values."""
    s = Settings.from_env({
        AGENT_ENV: "claude",
        "ANTHROPIC_API_KEY": "sk-secret-key-12345",
    })
    desc = s.describe()
    assert "sk-secret-key-12345" not in desc
    assert "agent=claude" in desc
    assert "anthropic_api_key=set" in desc
    assert "cursor_api_key=unset" in desc


# ── Agent command tests ──────────────────────────────────────────────────


def test_claude_command_shape():
    """Claude command includes -p, --cwd, --allowedTools, --output-format."""
    cmd, args, env = build_agent_command("claude", "fix bug", "/tmp/repo", None)
    assert cmd == "claude"
    assert "-p" in args
    assert "--cwd" in args
    assert "--allowedTools" in args


def test_omp_command_shape():
    """OMP command includes -p, --sandbox=NONE, --cwd."""
    cmd, args, env = build_agent_command("omp", "fix bug", "/tmp/repo", None)
    assert cmd == "gemini"
    assert "--sandbox=NONE" in args
    assert "--cwd" in args


def test_cursor_command_shape():
    """Cursor command includes --task, --dir."""
    cmd, args, env = build_agent_command("cursor", "fix bug", "/tmp/repo", None)
    assert cmd == "cursor-agent"
    assert "--task" in args
    assert "--dir" in args


# ── Wire round-trip tests ────────────────────────────────────────────────


def _child_env(**extra):
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


def test_initialize_and_tools_list_over_stdio():
    """Wire round-trip: initialize + list_tools succeeds."""
    init, listed, _ = asyncio.run(_round_trip("ok"))
    assert init.server_info.name == "mooshik-coder"
    tools = {tool.name: tool for tool in listed.tools}
    assert "delegate" in tools
    assert "check" in tools


def test_delegate_tool_call_returns_handle():
    """Wire round-trip: delegate returns a JSON object with a handle."""
    _, _, result = asyncio.run(
        _round_trip("ok", ("delegate", {"task": "fix bug", "repo": "/tmp"}))
    )
    data = json.loads(result.content[0].text)
    assert "handle" in data
    assert data["agent"] == "claude"


def test_check_tool_call_returns_status():
    """Wire round-trip: check returns a JSON object with status."""
    _, _, result = asyncio.run(
        _round_trip("ok", ("check", {"handle": "abc123"}))
    )
    data = json.loads(result.content[0].text)
    assert "status" in data


def test_upstream_failure_arrives_as_result():
    """Wire round-trip: a boom backend returns a contained error, not a crash."""
    _, _, result = asyncio.run(
        _round_trip("boom", ("delegate", {"task": "x", "repo": "/tmp"}))
    )
    text = result.content[0].text
    assert "failed" in text.lower() or "RuntimeError" in text


def test_hung_backend_answers_within_bound():
    """Wire round-trip: a hanging backend is answered by the timeout guard."""
    _, _, result = asyncio.run(
        _round_trip(
            "hang",
            ("delegate", {"task": "x", "repo": "/tmp"}),
            timeout_secs="0.2",
        )
    )
    assert "Timed out" in result.content[0].text


def test_stray_print_does_not_corrupt_framing():
    """Wire round-trip: a stray print() in the backend does not break JSON-RPC."""
    _, _, result = asyncio.run(
        _round_trip("noisy", ("delegate", {"task": "x", "repo": "/tmp"}))
    )
    text = result.content[0].text
    assert "handle" in text or "test01" in text
    assert "STRAY" not in text


# ── Backend tests ────────────────────────────────────────────────────────


def test_delegate_spawns_process(tmp_path):
    """Backend: delegate calls Popen and returns a handle."""
    backend = CoderBackend(agent="claude")
    fake_proc = FakeProcess(returncode=None, pid=99999)
    with patch("coder_mcp.backend.subprocess.Popen", return_value=fake_proc) as mock_popen:
        result = json.loads(backend.delegate("fix the bug", str(tmp_path)))
    assert "handle" in result
    assert result["agent"] == "claude"
    assert result["repo"] == str(tmp_path)
    mock_popen.assert_called_once()


def test_check_running_handle(tmp_path):
    """Backend: check on a running process returns status=running."""
    backend = CoderBackend(agent="claude")
    fake_proc = FakeProcess(returncode=None)
    backend._handles["abc"] = fake_proc
    result = json.loads(backend.check("abc"))
    assert result["status"] == "running"
    assert "abc" in backend._handles  # still tracked


def test_check_exited_handle(tmp_path):
    """Backend: check on an exited process returns status=exited with exit_code."""
    backend = CoderBackend(agent="claude")
    fake_proc = FakeProcess(returncode=0)
    backend._handles["def"] = fake_proc
    result = json.loads(backend.check("def"))
    assert result["status"] == "exited"
    assert result["exit_code"] == 0
    assert "def" not in backend._handles  # removed after exit


def test_check_unknown_handle():
    """Backend: check on an unknown handle returns status=unknown."""
    backend = CoderBackend(agent="claude")
    result = json.loads(backend.check("nonexistent"))
    assert result["status"] == "unknown"


def test_check_repeated_queries_return_exited(tmp_path):
    """Backend: check retains exit status across repeated queries."""
    backend = CoderBackend(agent="claude")
    fake_proc = FakeProcess(returncode=0)
    backend._handles["ghi"] = fake_proc

    first = json.loads(backend.check("ghi"))
    assert first["status"] == "exited"
    assert first["exit_code"] == 0

    second = json.loads(backend.check("ghi"))
    assert second["status"] == "exited"
    assert second["exit_code"] == 0


def test_stdin_is_devnull(tmp_path):
    """Backend: child agent process is spawned with stdin=subprocess.DEVNULL."""
    backend = CoderBackend(agent="cursor")
    fake_proc = FakeProcess(returncode=None, pid=12345)
    with patch("coder_mcp.backend.subprocess.Popen", return_value=fake_proc) as mock_popen:
        backend.delegate("write code", str(tmp_path))
    mock_popen.assert_called_once()
    _, kwargs = mock_popen.call_args
    assert kwargs.get("stdin") == subprocess.DEVNULL
    assert kwargs.get("stdout") == subprocess.DEVNULL
    assert kwargs.get("stderr") == subprocess.DEVNULL


def test_delegate_missing_binary_raises_tool_error(tmp_path):
    """Backend: missing agent binary raises CoderToolError naming the agent and binary."""
    backend = CoderBackend(agent="claude")
    with patch("coder_mcp.backend.subprocess.Popen", side_effect=FileNotFoundError):
        with pytest.raises(CoderToolError, match="CLI binary.*not found on PATH"):
            backend.delegate("fix bug", str(tmp_path))


def test_delegate_oserror_raises_tool_error(tmp_path):
    """Backend: OSError on spawn raises CoderToolError with exception type."""
    backend = CoderBackend(agent="omp")
    with patch("coder_mcp.backend.subprocess.Popen", side_effect=OSError("Permission denied")):
        with pytest.raises(CoderToolError, match="Could not spawn"):
            backend.delegate("fix bug", str(tmp_path))


def test_secret_redaction_on_tool_egress():
    """Tool surface: secrets present in CoderBackend are redacted from error output."""
    from coder_mcp.tools import guarded
    backend = CoderBackend(
        agent="claude",
        env={"ANTHROPIC_API_KEY": "sk-sensitive-secret-token-xyz"},
    )
    assert backend.secrets == ("sk-sensitive-secret-token-xyz",)

    def failing_call():
        raise CoderToolError("Failure with token sk-sensitive-secret-token-xyz in path")

    result = asyncio.run(
        guarded("test_redact", failing_call, timeout_secs=5.0, secrets=backend.secrets)
    )
    assert "sk-sensitive-secret-token-xyz" not in result
    assert "[redacted]" in result


def test_missing_repo_fails_closed():
    """Backend: delegate with a non-existent repo raises CoderToolError."""
    backend = CoderBackend(agent="claude")
    with pytest.raises(CoderToolError, match="does not exist"):
        backend.delegate("fix bug", "/nonexistent/path/that/does/not/exist")


# ── Standing rule tests ─────────────────────────────────────────────────


def test_standing_rule_written(tmp_path):
    """Standing rule: AGENTS.md is written to the repo root."""
    path = write_standing_rule(str(tmp_path))
    assert path is not None
    agents_md = Path(path)
    assert agents_md.exists()
    content = agents_md.read_text()
    assert "Standing Constraints" in content
    assert "lambo_recall" in content


def test_standing_rule_missing_repo():
    """Standing rule: returns None when repo does not exist."""
    path = write_standing_rule("/nonexistent/path/that/does/not/exist")
    assert path is None


# ── Entrypoint tests ────────────────────────────────────────────────────


def _run_entrypoint(env, args=()):
    return subprocess.run(
        [sys.executable, str(ROOT / "server.py"), *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=60,
    )


def test_missing_credentials_exit_nonzero():
    """Entrypoint: missing MOOSHIK_CODER_AGENT exits 2 with message on stderr."""
    done = _run_entrypoint(_child_env())
    assert done.returncode == 2
    assert done.stdout == ""
    assert AGENT_ENV in done.stderr


def test_args_rejected():
    """Entrypoint: passing arguments exits 2."""
    done = _run_entrypoint(
        _child_env(MOOSHIK_CODER_AGENT="claude"),
        args=["--unexpected"],
    )
    assert done.returncode == 2
    assert done.stdout == ""
    assert "no arguments" in done.stderr.lower() or "takes no arguments" in done.stderr.lower()


# ------------------------------------------------------- the --agent argument ----
# `[mcp_servers.*.env]` values are resolved by Mooshik as vault secret NAMES
# (mcp_host::resolve_env), so a literal `MOOSHIK_CODER_AGENT = "claude"` there
# made the host look up a secret called `claude`, fail, and refuse to spawn this
# server — `mooshik configure coder` wrote a config that could not start it.
# The agent name is not a secret, so it travels as an argument instead.


def test_the_agent_argument_wins_over_the_environment():
    from coder_mcp.config import Settings

    s = Settings.from_env({"MOOSHIK_CODER_AGENT": "omp"}, agent_override="claude")
    assert s.agent == "claude"


def test_the_environment_still_works_for_a_direct_invocation():
    """Kept so `MOOSHIK_CODER_AGENT=claude python3 -m coder_mcp` still runs."""
    from coder_mcp.config import Settings

    assert Settings.from_env({"MOOSHIK_CODER_AGENT": "omp"}).agent == "omp"


def test_neither_source_fails_closed_and_names_both():
    from coder_mcp.config import ConfigError, Settings

    with pytest.raises(ConfigError) as caught:
        Settings.from_env({})
    message = str(caught.value)
    assert "--agent" in message
    assert "MOOSHIK_CODER_AGENT" in message
    # The old text sent people to env under [mcp_servers.coder], which is the
    # exact configuration that cannot work.
    assert "NOT in env" in message


def test_an_unknown_agent_is_still_rejected_when_passed_as_an_argument():
    from coder_mcp.config import ConfigError, Settings

    with pytest.raises(ConfigError):
        Settings.from_env({}, agent_override="notanagent")


@pytest.mark.parametrize("argv", [["--agent"], ["--agent", "claude", "extra"], ["--rogue"]])
def test_main_refuses_anything_that_is_not_a_lone_agent_flag(argv):
    """The no-CLI-surface rule bends for exactly one non-secret argument and
    no further: a secret passed as an argument is visible in `ps`."""
    from coder_mcp.__main__ import main

    assert main(argv) == 2


@pytest.mark.parametrize("argv", [["--agent", "claude"], ["--agent=claude"]])
def test_both_agent_spellings_reach_settings(argv):
    """Both `--agent X` and `--agent=X` must arrive as the same override.

    Asserted at the seam rather than through `main`'s return, because a server
    with a valid agent and no credential starts and serves stdio — a credential
    is not required until an agent is actually spawned — so the return code
    says nothing about how the argument was parsed.
    """
    import coder_mcp.__main__ as entry

    seen = {}

    def capture(env=None, agent_override=None):
        seen["agent"] = agent_override
        raise entry.ConfigError("stop here")

    with patch.object(entry.Settings, "from_env", staticmethod(capture)):
        assert entry.main(argv) == 2
    assert seen["agent"] == "claude"
