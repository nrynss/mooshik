# M12g Adversarial Review: Round 1

**Verdict:** REMEDIATE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| All source files under `mcp-servers/coder/` | True | All files located under `mcp-servers/coder/` with exact dependency pins and clean structure. |
| 24 tests pass offline | True | `pytest mcp-servers/coder/tests -q` passes 24 tests without network or credentials. |
| Non-blocking fire-and-forget `delegate` (<60s) | True | `CoderBackend.delegate` spawns agent subprocess via `subprocess.Popen` and returns handle immediately. |
| Standing rule `AGENTS.md` written before spawn | True | `write_standing_rule(repo)` writes constraint instructions to repo root prior to launching the agent. |
| Support for all 3 agents (`claude`, `omp`, `cursor`) | **FALSE** | `CURSOR_API_KEY` is omitted from `coder_mcp/config.py` and `coder_mcp/__main__.py`. The `cursor` agent is launched without credentials. |
| Child process stdio stream isolation | **FALSE** | `backend.py` sets `stdout` and `stderr` to `DEVNULL` but omits `stdin=subprocess.DEVNULL`, causing background agents to inherit the MCP server's JSON-RPC stdio channel. |
| Secret redaction on tool error egress | **FALSE** | `CoderBackend` does not expose a `secrets` attribute or pass configured secrets, causing `tools.py` to use an empty tuple and bypass redaction. |
| `mooshik configure coder` non-interactive CLI | **FALSE** | `configure_coder` calls `secret::read_secret_value()` when secret is absent from vault, which blocks indefinitely on stdin if `MOOSHIK_SECRET_VALUE` is unset and stdin is an interactive TTY. |
| `check` status reporting | **FALSE** | `CoderBackend.check` immediately deletes handles upon process exit (`del self._handles[handle]`), causing any subsequent `check` query on the same handle to report `unknown`. |

## Gate Results

- `pytest mcp-servers/coder/tests -q`: PASS (24 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (Max Python file is `test_coder.py` at 316 lines)

## Findings

| # | Priority | File | Finding with remediation |
| :--- | :--- | :--- | :--- |
| 1 | P1 | `mcp-servers/coder/coder_mcp/config.py`, `coder_mcp/__main__.py` | **`CURSOR_API_KEY` credential is dropped and never forwarded.** `src/cli/configure.rs` configures `CURSOR_API_KEY = "cursor-api-key"` under `[mcp_servers.coder.env]`. However, `coder_mcp/config.py` does not define `CURSOR_API_KEY_ENV` or a `cursor_api_key` field in `Settings`, and `__main__.py` does not add it to `passthrough_env`. When delegating to `cursor`, the agent process receives no API key. *Remediation: Add `CURSOR_API_KEY_ENV = "CURSOR_API_KEY"` and `cursor_api_key: str | None` to `Settings`, parse it in `Settings.from_env()`, and forward `CURSOR_API_KEY` in `__main__.py`'s `passthrough_env`.* |
| 2 | P1 | `mcp-servers/coder/coder_mcp/backend.py` | **Background agent process inherits parent's JSON-RPC stdin.** In `CoderBackend.delegate`, `subprocess.Popen` specifies `stdout=subprocess.DEVNULL` and `stderr=subprocess.DEVNULL` but leaves `stdin` unspecified (`None`). The child process inherits the MCP server's stdin file descriptor (which is the stdio pipe connected to Mooshik's JSON-RPC host), creating a race where background agents can read MCP frames and corrupt protocol synchronization. *Remediation: Set `stdin=subprocess.DEVNULL` explicitly in `subprocess.Popen`.* |
| 3 | P1 | `mcp-servers/coder/coder_mcp/backend.py`, `coder_mcp/tools.py` | **Secret redaction bypassed due to missing `secrets` attribute on `CoderBackend`.** `tools.py` expects `getattr(backend, "secrets", ())`, but `CoderBackend` does not set `self.secrets`. As a result, `secrets` in `tools.py` is always an empty tuple, and any API keys present in `CoderToolError` or exception strings will not be redacted before returning across the wire. *Remediation: Store `self.secrets = tuple(v for v in self.env.values() if v)` in `CoderBackend.__init__`.* |
| 4 | P2 | `src/cli/configure.rs`, `src/text/en.toml` | **`mooshik configure coder` hangs on stdin when `MOOSHIK_SECRET_VALUE` is unset.** Line 147 of `src/cli/configure.rs` calls `secret::read_secret_value()`, which falls back to `io::stdin().read_to_end()` when `MOOSHIK_SECRET_VALUE` is unset. In an interactive terminal session where the secret is not yet in the vault, this blocks silently on stdin waiting for EOF. Furthermore, `coder_prompt_*` keys in `src/text/en.toml` are never referenced. *Remediation: Only attempt to store the secret during `configure_coder` if `env::var("MOOSHIK_SECRET_VALUE")` is explicitly present, or remove the silent stdin `read_to_end` fallback in favor of `mooshik secret set`.* |
| 5 | P2 | `mcp-servers/coder/coder_mcp/backend.py` | **`check` deletes finished handles immediately, causing repeated queries to report `unknown`.** Lines 141-143 delete the handle from `self._handles` upon the first poll after exit. If the model or operator polls the handle again to confirm its exit status, it receives `{"status": "unknown"}`. *Remediation: Retain finished process status (e.g. record `(status="exited", exit_code=exit_code)` in a bounded exit history) so subsequent checks truthfully report `exited`.* |
| 6 | P3 | `mcp-servers/coder/coder_mcp/backend.py` | **Child agent processes not bound to parent lifecycle with `PR_SET_PDEATHSIG`.** Per `PLAN.md` lines 1018-1022 ("Bound the child at spawn rather than relying on cleanup; a killed parent never runs its own teardown"), agent processes spawned on Linux without `PR_SET_PDEATHSIG` will be reparented to PID 1 if the MCP server is forcefully terminated (SIGKILL) rather than exiting with the parent. *Remediation: Add a `preexec_fn` on Linux platforms to set `PR_SET_PDEATHSIG = SIGTERM` via `ctypes.CDLL(None).prctl(1, 15)`.* |
| 7 | P3 | `README.md` | **Documentation references invalid CLI command `mooshik permissions list`.** `README.md` line 178 lists `mooshik permissions list`, which fails with exit code 2 (`unexpected argument 'list' found`). *Remediation: Change `mooshik permissions list` to `mooshik permissions` in `README.md`.* |
| 8 | P3 | `mcp-servers/coder/tests/test_coder.py` | **Test coverage gaps for agent errors and secret redaction.** `test_coder.py` lacks unit tests for `FileNotFoundError` (missing CLI binary on PATH), `OSError` spawn failures, and secret redaction on tool egress. *Remediation: Add unit tests verifying `FileNotFoundError` error handling and secret redaction.* |

## Notes for Next Round

Remediate all P1, P2, and P3 findings:
1. Add `CURSOR_API_KEY` handling across `config.py` and `__main__.py`.
2. Add `stdin=subprocess.DEVNULL` to `subprocess.Popen` in `backend.py`.
3. Add `self.secrets` to `CoderBackend` for tool egress redaction.
4. Prevent `configure_coder` from blocking on stdin when `MOOSHIK_SECRET_VALUE` is unset.
5. Retain exited handle state in `backend.py` so repeated `check` calls return `exited`.
6. Bind child lifecycle to parent on Linux via `PR_SET_PDEATHSIG`.
7. Fix `mooshik permissions` in `README.md`.
8. Expand `test_coder.py` coverage for missing binary errors and secret redaction.
