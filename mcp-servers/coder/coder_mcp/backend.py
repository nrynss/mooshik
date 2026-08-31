"""The subprocess management seam: spawn coding agents, track handles.

**The seam.** ``CoderBackend`` manages agent processes through
``subprocess.Popen`` and stores them by a short hex handle. ``delegate``
spawns an agent and returns immediately; ``check`` polls its status. This
is the shape ``MCP_CALL_WAIT`` (60s) demands: a code change can take
minutes, so a blocking call is a design that times out by construction.

**The daemon boundary holds.** MCP servers are children of the host, and
agents spawned here are children of the server. When the pane closes,
the server dies, and its children die with it — which is also the
behaviour you want, since an agent editing a repository after the user
closed Mooshik is exactly the thing M12 refused to build.

**The result arrives the ambient way.** M12d's workspace watcher sees the
edits land and derives them, so the pane fills with what the contractor
did while it is still working. Nothing has to marshal a diff back through
a tool call.
"""

from __future__ import annotations

import ctypes
import json
import logging
import signal
import subprocess
import sys
import uuid
from pathlib import Path

from .agents import build_agent_command
from .errors import CoderToolError
from .standing_rule import write_standing_rule

log = logging.getLogger(__name__)


def _pdeathsig_preexec() -> None:
    """Set PR_SET_PDEATHSIG to SIGTERM so the child exits if the parent dies."""
    try:
        libc = ctypes.CDLL(None)
        libc.prctl(1, signal.SIGTERM)
    except Exception:  # noqa: BLE001
        pass


class CoderBackend:
    """Spawn and monitor coding agent subprocesses.

    Attributes
    ----------
    agent:
        Which coding agent to delegate to (``"claude"``, ``"omp"``,
        ``"cursor"``, ``"agy"``).
    env:
        Additional environment variables to pass to the agent (credentials).
    secrets:
        Credential values to redact from any tool error output.
    """

    def __init__(self, agent: str, env: dict[str, str] | None = None) -> None:
        self.agent = agent
        self.env: dict[str, str] = env or {}
        self.secrets: tuple[str, ...] = tuple(v for v in self.env.values() if v)
        self._handles: dict[str, subprocess.Popen] = {}  # type: ignore[type-arg]
        self._exited: dict[str, int] = {}

    def delegate(self, task: str, repo: str) -> str:
        """Spawn the configured coding agent and return immediately.

        This is a synchronous method called via ``guarded``/``to_thread``
        in the MCP tool surface. It validates the repo, writes the
        standing rule, builds the agent command, spawns the process, and
        returns a JSON object with the handle ID, agent name, repo, and
        task summary.

        Returns
        -------
        str
            A JSON object: ``{"handle": "...", "agent": "...", "repo": "...",
            "task": "..."}``.

        Raises
        ------
        CoderToolError
            If the repo does not exist or is not a directory.
        """
        repo_path = Path(repo)
        if not repo_path.is_dir():
            raise CoderToolError(
                f"Repository path does not exist or is not a directory: {repo}"
            )

        # Write the standing rule before spawning — the agent's task prompt
        # refers to it.
        standing_rule_path = write_standing_rule(repo)

        command, args, child_env = build_agent_command(
            self.agent,
            task,
            repo,
            standing_rule_path,
            env_overrides=self.env,
        )

        log.info(
            "delegating to %s: command=%s repo=%s", self.agent, command, repo
        )

        preexec = _pdeathsig_preexec if sys.platform.startswith("linux") else None

        try:
            process = subprocess.Popen(
                [command, *args],
                cwd=repo,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                env=child_env,
                preexec_fn=preexec,
            )
        except FileNotFoundError:
            raise CoderToolError(
                f"The {self.agent} CLI binary ({command!r}) was not found on "
                f"PATH. Install it and try again."
            ) from None
        except OSError as exc:
            raise CoderToolError(
                f"Could not spawn the {self.agent} agent: {type(exc).__name__}"
            ) from None

        handle = uuid.uuid4().hex[:8]
        self._handles[handle] = process
        log.info("spawned %s with handle %s (pid %d)", self.agent, handle, process.pid)

        return json.dumps({
            "handle": handle,
            "agent": self.agent,
            "repo": repo,
            "task": task,
        })

    def check(self, handle: str) -> str:
        """Check the status of a previously delegated agent.

        Returns
        -------
        str
            A JSON object with at least ``handle`` and ``status`` keys.
            Status is one of ``"running"``, ``"exited"``, or ``"unknown"``.
        """
        if handle in self._exited:
            return json.dumps({
                "handle": handle,
                "status": "exited",
                "exit_code": self._exited[handle],
            })

        process = self._handles.get(handle)
        if process is None:
            return json.dumps({"handle": handle, "status": "unknown"})

        exit_code = process.poll()
        if exit_code is None:
            return json.dumps({"handle": handle, "status": "running"})

        # Process has exited — record in exited history and remove from handles.
        del self._handles[handle]
        if len(self._exited) >= 1000:
            del self._exited[next(iter(self._exited))]
        self._exited[handle] = exit_code
        return json.dumps({
            "handle": handle,
            "status": "exited",
            "exit_code": exit_code,
        })
