import asyncio
import os
import subprocess
import sys
from pathlib import Path
import pytest

from artifacts_mcp.config import Settings, ConfigError, API_KEY_ENV, PROJECT_ENV
from artifacts_mcp.secretscan import find_secret

ROOT = Path(__file__).resolve().parents[1]

def test_missing_credentials_fails_closed():
    with pytest.raises(ConfigError) as caught:
        Settings.from_env({})
    assert API_KEY_ENV in str(caught.value) and PROJECT_ENV in str(caught.value)

def test_secretscan_finds_secrets():
    assert find_secret("xoxb-1234567890-1234567890-a1b2c3d4e5f6g7h8i9j0") == "slack-token"
    assert find_secret("my password is SECRET = 'A1b2C3d4E5f6G7h8I9j0K+'") == "generic-assignment"
    assert find_secret("nothing here") is None
    assert find_secret("my vault pass is XYZ", extra_forbidden=("XYZ",)) == "vault-value"

def _child_env(**extra):
    env = {"PATH": os.environ.get("PATH", ""), "HOME": os.environ.get("HOME", ""), "PYTHONPATH": str(ROOT)}
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
    init, listed, _ = asyncio.run(_round_trip("ok"))
    assert init.server_info.name == "mooshik-artifacts"
    tools = {tool.name: tool for tool in listed.tools}
    assert "extract_concepts" in tools
    
def test_tool_call_returns_backend_text():
    _, _, result = asyncio.run(_round_trip("ok", ("extract_concepts", {"file_path": "/tmp/test.png"})))
    assert "canned" in result.content[0].text
    
def test_upstream_failure_arrives_as_result():
    _, _, result = asyncio.run(_round_trip("boom", ("extract_concepts", {"file_path": "/tmp/x"})))
    assert "upstream failed" in result.content[0].text or "failed upstream" in result.content[0].text
    
def test_hung_backend_answers_within_bound():
    _, _, result = asyncio.run(_round_trip("hang", ("extract_concepts", {"file_path": "/tmp/x"}), timeout_secs="0.2"))
    assert "Timed out" in result.content[0].text
    
def test_stray_print_does_not_corrupt_framing():
    _, _, result = asyncio.run(_round_trip("noisy", ("extract_concepts", {"file_path": "/tmp/x"})))
    assert "canned" in result.content[0].text
    assert "STRAY" not in result.content[0].text

def _run_entrypoint(env, args=()):
    return subprocess.run([sys.executable, str(ROOT / "server.py"), *args], env=env, capture_output=True, text=True, timeout=60)

def test_missing_credentials_exit_nonzero():
    done = _run_entrypoint(_child_env())
    assert done.returncode == 2
    assert done.stdout == ""
    assert PROJECT_ENV in done.stderr and API_KEY_ENV in done.stderr

from artifacts_mcp.backend import ArtifactsBackend
from artifacts_mcp.errors import ArtifactsToolError
from fakes import FakeClient, FakeResponse
import json

def test_extract_image_success(tmp_path):
    p = tmp_path / "test.png"
    p.write_bytes(b"fake image data")
    
    responses = [FakeResponse('[{"content": "a cat", "concept_type": "entity"}]')]
    backend = ArtifactsBackend(FakeClient(responses))
    
    result = backend.extract(str(p))
    data = json.loads(result)
    assert "concepts" in data
    assert data["concepts"][0]["content"] == "a cat"

def test_extract_audio_success(tmp_path):
    p = tmp_path / "test.mp3"
    p.write_bytes(b"fake audio data")
    
    responses = [FakeResponse('[{"content": "beep", "concept_type": "observation"}]')]
    backend = ArtifactsBackend(FakeClient(responses))
    
    result = backend.extract(str(p))
    data = json.loads(result)
    assert data["concepts"][0]["content"] == "beep"

def test_extract_drops_on_secret(tmp_path):
    p = tmp_path / "test.png"
    p.write_bytes(b"fake image data")
    
    responses = [FakeResponse('found secret xoxb-1234567890-1234567890-a1b2c3d4e5f6g7h8i9j0')]
    backend = ArtifactsBackend(FakeClient(responses))
    
    result = backend.extract(str(p))
    data = json.loads(result)
    assert "error" in data
    assert "secret detected" in data["error"]

def test_extract_drops_on_vault_value(tmp_path):
    p = tmp_path / "test.png"
    p.write_bytes(b"fake image data")
    
    responses = [FakeResponse('the password is vaultpass')]
    backend = ArtifactsBackend(FakeClient(responses), secrets=["vaultpass"])
    
    result = backend.extract(str(p))
    data = json.loads(result)
    assert "error" in data
    assert "secret detected" in data["error"]

def test_extract_missing_file():
    backend = ArtifactsBackend(FakeClient([]))
    with pytest.raises(ArtifactsToolError, match="File not found"):
        backend.extract("/tmp/does_not_exist_12345.png")

def test_extract_unsupported_file(tmp_path):
    p = tmp_path / "test.txt"
    p.write_text("hello")
    backend = ArtifactsBackend(FakeClient([]))
    with pytest.raises(ArtifactsToolError, match="Unsupported file type"):
        backend.extract(str(p))
