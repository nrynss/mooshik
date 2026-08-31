# M12g Remediation: Round 1

## Fixes Applied

| # | Review Finding | File | Change Made |
| :--- | :--- | :--- | :--- |
| 1 | `CURSOR_API_KEY` credential dropped | `mcp-servers/coder/coder_mcp/config.py`, `mcp-servers/coder/coder_mcp/__main__.py` | Added `CURSOR_API_KEY_ENV = "CURSOR_API_KEY"`, `cursor_api_key: str | None = None` to `Settings`, parsed in `Settings.from_env()`, included in `describe()`, and forwarded in `passthrough_env` in `__main__.py`. |
| 2 | Background agent process inherits JSON-RPC stdin | `mcp-servers/coder/coder_mcp/backend.py` | Explicitly set `stdin=subprocess.DEVNULL` in `subprocess.Popen`. |
| 3 | Missing `secrets` on `CoderBackend` breaks egress redaction | `mcp-servers/coder/coder_mcp/backend.py` | Initialized `self.secrets = tuple(v for v in self.env.values() if v)` on `CoderBackend` to supply configured secret values to `tools.guarded()`. |
| 4 | `configure_coder` blocks on stdin | `src/cli/configure.rs`, `src/text/en.toml` | Updated `configure_coder` to store vault secrets only when `MOOSHIK_SECRET_VALUE` is set in the environment, preventing blocking on interactive stdin. Cleaned up unused `coder_prompt_*` keys from `src/text/en.toml`. |
| 5 | `check` deletes finished handles immediately | `mcp-servers/coder/coder_mcp/backend.py` | Added a bounded `_exited: dict[str, int]` history in `CoderBackend` so subsequent `check` polls on an exited handle return `status: "exited"` with exit code. |
| 6 | Daemon boundary with `PR_SET_PDEATHSIG` on Linux | `mcp-servers/coder/coder_mcp/backend.py` | Implemented `_pdeathsig_preexec()` using `ctypes.CDLL(None).prctl(1, signal.SIGTERM)` and passed as `preexec_fn` when running on Linux. |
| 7 | `mooshik permissions list` typo in `README.md` | `README.md` | Fixed CLI reference table command entry from `mooshik permissions list` to `mooshik permissions`. |
| 8 | Test coverage gaps | `mcp-servers/coder/tests/test_coder.py` | Added 6 tests: `test_cursor_api_key_configured_and_forwarded`, `test_delegate_missing_binary_raises_tool_error`, `test_delegate_oserror_raises_tool_error`, `test_check_repeated_queries_return_exited`, `test_secret_redaction_on_tool_egress`, and `test_stdin_is_devnull`. |

## Gate Results After Remediation

- `pytest mcp-servers/coder/tests -q`: PASS (30 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS

## Summary

8 findings remediated. All gate tests pass.
