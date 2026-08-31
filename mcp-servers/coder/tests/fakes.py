"""Test doubles for the coder MCP server.

``ScriptedBackend`` returns canned answers for ``delegate`` and ``check``,
exactly as the artifacts server's ``ScriptedBackend`` does for ``extract``.
``FakeProcess`` mocks ``subprocess.Popen`` with a configurable ``poll()``.
"""

from __future__ import annotations

from typing import Any


class ScriptedBackend:
    """A backend that returns canned answers, for wire-level round-trip tests.

    Parameters
    ----------
    delegate_answer:
        The string ``delegate()`` returns.
    check_answer:
        The string ``check()`` returns.
    boom:
        If set, both methods raise this exception instead of returning.
    """

    def __init__(
        self,
        delegate_answer: str = '{"handle":"test01","agent":"claude","repo":"/tmp","task":"test"}',
        check_answer: str = '{"handle":"test01","status":"running"}',
        secrets: tuple[str, ...] = (),
        boom: Exception | None = None,
    ) -> None:
        self.delegate_answer = delegate_answer
        self.check_answer = check_answer
        self.secrets = secrets
        self.boom = boom
        self.calls: list[tuple[str, Any]] = []

    def delegate(self, task: str, repo: str) -> str:
        self.calls.append(("delegate", (task, repo)))
        if self.boom is not None:
            raise self.boom
        return self.delegate_answer

    def check(self, handle: str) -> str:
        self.calls.append(("check", handle))
        if self.boom is not None:
            raise self.boom
        return self.check_answer


class FakeProcess:
    """A mock ``subprocess.Popen`` with a configurable ``poll()``.

    Parameters
    ----------
    returncode:
        If ``None``, ``poll()`` returns ``None`` (still running).
        If an int, ``poll()`` returns that exit code.
    pid:
        The process ID to report.
    """

    def __init__(self, returncode: int | None = None, pid: int = 12345) -> None:
        self._returncode = returncode
        self.pid = pid

    def poll(self) -> int | None:
        return self._returncode
