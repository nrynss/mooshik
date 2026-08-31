"""Agent command builders for the four supported coding agents.

Each function returns a triple ``(command, args, env)`` that
``subprocess.Popen`` can consume directly. The triple keeps the spawn site
in ``backend.py`` ignorant of agent-specific flags.

The task description is the user's original task wrapped in a template that
instructs the agent to read ``AGENTS.md`` first and consult Lambo memory
for constraints. This is the mechanism that ``video-shoot.md`` proved
works: a standing rule the agent reads at startup, not instructions
injected into the MCP server's own ``initialize`` message.
"""

from __future__ import annotations

import os
from typing import Sequence

#: The task preamble every agent receives, with ``{task}`` and
#: ``{standing_rule}`` placeholders.
_TASK_TEMPLATE = (
    "Read AGENTS.md in the repository root first — it contains standing "
    "constraints from the workspace memory graph. Consult Lambo memory via "
    "MCP for constraints on files and concepts you plan to change.\n\n"
    "TASK:\n{task}"
)

#: Preamble variant when a standing rule was written successfully.
_TASK_WITH_RULE = (
    "A standing rule has been written to {standing_rule}. "
    "Read it before making any change.\n\n"
    "{base}"
)


def _format_task(task: str, standing_rule_path: str | None) -> str:
    """Build the full task text from the user's task and optional rule path."""
    base = _TASK_TEMPLATE.format(task=task)
    if standing_rule_path:
        return _TASK_WITH_RULE.format(standing_rule=standing_rule_path, base=base)
    return base


def build_agent_command(
    agent: str,
    task: str,
    repo: str,
    standing_rule_path: str | None,
    *,
    env_overrides: dict[str, str] | None = None,
) -> tuple[str, list[str], dict[str, str]]:
    """Dispatch to the agent-specific command builder.

    Returns ``(command, args, env)`` for ``subprocess.Popen``.  The ``env``
    dict is the *complete* environment for the child — only the variables the
    agent needs are forwarded.

    Parameters
    ----------
    agent:
        One of ``"claude"``, ``"omp"``, ``"cursor"``, ``"agy"``.
    task:
        The user's original task description.
    repo:
        Absolute path to the repository the agent should work in.
    standing_rule_path:
        Path to the ``AGENTS.md`` file written by ``write_standing_rule``,
        or ``None`` if it could not be written.
    env_overrides:
        Additional environment variables to inject (credentials, etc.).
    """
    overrides = env_overrides or {}
    if agent == "claude":
        return _claude_command(task, repo, standing_rule_path, overrides)
    if agent == "omp":
        return _omp_command(task, repo, standing_rule_path, overrides)
    if agent == "cursor":
        return _cursor_command(task, repo, standing_rule_path, overrides)
    if agent == "agy":
        return _agy_command(task, repo, standing_rule_path, overrides)
    # Unreachable if config validated, but fail closed anyway.
    raise ValueError(f"unknown agent: {agent!r}")


def _base_env(overrides: dict[str, str]) -> dict[str, str]:
    """Build a minimal child environment from PATH + overrides.

    We deliberately do **not** inherit the full ``os.environ``: the child is
    a coding agent that edits a repo, and forwarding everything this process
    sees (including vault-resolved secrets for other servers) would leak them
    into the agent's environment.
    """
    env: dict[str, str] = {}
    # PATH is always needed so the child can find its own binaries.
    path = os.environ.get("PATH")
    if path:
        env["PATH"] = path
    # HOME is needed for the agent's own config files.
    home = os.environ.get("HOME")
    if home:
        env["HOME"] = home
    env.update(overrides)
    return env


def _claude_command(
    task: str,
    repo: str,
    standing_rule_path: str | None,
    overrides: dict[str, str],
) -> tuple[str, list[str], dict[str, str]]:
    """Claude Code: ``claude -p "..." --cwd <repo> --allowedTools "..." --output-format stream-json``."""
    prompt = _format_task(task, standing_rule_path)
    args = [
        "-p", prompt,
        "--cwd", repo,
        "--allowedTools", "Edit,Write,Bash",
        "--output-format", "stream-json",
    ]
    return "claude", args, _base_env(overrides)


def _omp_command(
    task: str,
    repo: str,
    standing_rule_path: str | None,
    overrides: dict[str, str],
) -> tuple[str, list[str], dict[str, str]]:
    """Gemini CLI (OMP): ``gemini -p "..." --sandbox=NONE --cwd <repo>``."""
    prompt = _format_task(task, standing_rule_path)
    args = [
        "-p", prompt,
        "--sandbox=NONE",
        "--cwd", repo,
    ]
    return "gemini", args, _base_env(overrides)


def _cursor_command(
    task: str,
    repo: str,
    standing_rule_path: str | None,
    overrides: dict[str, str],
) -> tuple[str, list[str], dict[str, str]]:
    """Cursor Agent CLI: ``cursor-agent --task "..." --dir <repo>``."""
    prompt = _format_task(task, standing_rule_path)
    args = [
        "--task", prompt,
        "--dir", repo,
    ]
    return "cursor-agent", args, _base_env(overrides)


def _agy_command(
    task: str,
    repo: str,
    standing_rule_path: str | None,
    overrides: dict[str, str],
) -> tuple[str, list[str], dict[str, str]]:
    """Antigravity (Google coding agent): ``agy -p "..." --dangerously-skip-permissions``."""
    prompt = _format_task(task, standing_rule_path)
    args = [
        "-p", prompt,
        "--dangerously-skip-permissions",
    ]
    return "agy", args, _base_env(overrides)
