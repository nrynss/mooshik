# M12g Adversarial Review: Round 4

**Verdict:** APPROVE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| All Round 3 findings remediated | True | Verified in `.github/workflows/ci.yml:195-206`, `README.md:47, 150`, and `dev-diary/PLAN.md:71`. |
| CI executes coder MCP tests | True | `.github/workflows/ci.yml:195-206` adds the `coder-mcp` job with SHA-pinned actions and exact dependency pins (`pytest==9.1.1`, `mcp==2.1.1`). |
| README and PLAN summaries fully in sync | True | `README.md` diagram and overview list all 4 supported agents (`Claude / OMP / Cursor / Antigravity`), and `dev-diary/PLAN.md:71` accurately reflects 4 agents and 32 offline tests. |
| Non-blocking fire-and-forget `delegate` (<60s) | True | `backend.py:114-142` spawns subprocess via `subprocess.Popen` with `DEVNULL` streams and returns JSON handle immediately. |
| Daemon boundary holds on Linux | True | `backend.py:39-45, 111, 121` sets `PR_SET_PDEATHSIG = SIGTERM` via `preexec_fn` and redirects stdio to `DEVNULL`. |
| Permission grant is `prompt`, never `allow` | True | `src/cli/configure.rs:207, 211` sets `"mcp.coder.*" = "prompt"`. |
| Secrets handled via vault refs in config | True | `src/cli/configure.rs:216-218` sets secret names in `[mcp_servers.coder.env]`; secrets redacted on egress via `tools.py:guarded`. |
| Standing rule `AGENTS.md` written before spawn | True | `backend.py:97` invokes `write_standing_rule(repo)` prior to `subprocess.Popen`. |
| `check` status reporting (running/exited/unknown) | True | `backend.py:153-177` checks bounded `_exited` map, active `_handles`, and process poll state. |
| Support for all 4 agents (`claude`, `omp`, `cursor`, `agy`) | True | `VALID_AGENTS` in `config.py:32`, `build_agent_command` in `agents.py:73-80`, and clap parser in `command.rs:154`. |

## Gate Results

- `pytest mcp-servers/coder/tests -q`: PASS (32 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (All `.rs` files <= 1500 lines)

## Findings

| # | Priority | File | Finding with remediation |
| :--- | :--- | :--- | :--- |
| — | — | — | *No findings. All previous findings fully remediated.* |

## Notes for Next Round

None. M12g is complete, fully tested (32 offline tests), compliant with all daemon and timeout boundaries, and properly integrated into CLI, CI workflows, and documentation.
