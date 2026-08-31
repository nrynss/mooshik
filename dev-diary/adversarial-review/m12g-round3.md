# M12g Adversarial Review: Round 3

**Verdict:** REMEDIATE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| All 4 Round 2 findings remediated | True | Verified in `mcp-servers/coder/README.md`, `docs/src/content/docs/cli.md`, `mcp-host.md`, `__init__.py`, `config.py`, `agents.py`, `backend.py`, and `tools.py`. |
| Non-blocking fire-and-forget `delegate` (<60s) | True | `backend.py:114-142` spawns subprocess via `subprocess.Popen` with `DEVNULL` streams and returns JSON handle immediately. |
| Daemon boundary holds on Linux | True | `backend.py:39-45, 111, 121` sets `PR_SET_PDEATHSIG = SIGTERM` via `preexec_fn` and redirects stdio to `DEVNULL`. |
| Permission grant is `prompt`, never `allow` | True | `src/cli/configure.rs:207, 211` sets `"mcp.coder.*" = "prompt"`. |
| Secrets handled via vault refs in config | True | `src/cli/configure.rs:216-218` sets secret names in `[mcp_servers.coder.env]`; secrets redacted on egress via `tools.py:guarded`. |
| Standing rule `AGENTS.md` written before spawn | True | `backend.py:97` invokes `write_standing_rule(repo)` prior to `subprocess.Popen`. |
| `check` status reporting (running/exited/unknown) | True | `backend.py:153-177` checks bounded `_exited` map, active `_handles`, and process poll state. |
| Support for all 4 agents (`claude`, `omp`, `cursor`, `agy`) | True | `VALID_AGENTS` in `config.py:32`, `build_agent_command` in `agents.py:73-80`, and clap parser in `command.rs:154`. |
| CI executes coder MCP tests | **FALSE** | `.github/workflows/ci.yml` includes jobs for `news-mcp` and `artifacts-mcp`, but omits `coder-mcp`. The 32 tests in `mcp-servers/coder/tests` never run in GitHub Actions CI. |
| Agent documentation complete in README.md | **FALSE** | `README.md:47, 150` lists `(Claude / OMP / Cursor)` and `(Claude Code, Gemini CLI, or Cursor Agent)`, omitting `agy` (Antigravity). |
| Milestone plan summary accurate | **FALSE** | `dev-diary/PLAN.md:71` summary table records 24 offline tests and 3 agents instead of 32 offline tests and 4 agents (`claude`, `omp`, `cursor`, `agy`). |

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
| 1 | P3 | `.github/workflows/ci.yml` | **Missing `coder-mcp` CI job in `.github/workflows/ci.yml`.** `.github/workflows/ci.yml` includes dedicated offline test jobs for all other components (`ingester`, `mooshik-common`, `news-mcp`, `measurement`, `artifacts-mcp`), but omits `coder-mcp`. As a result, the 32 offline tests under `mcp-servers/coder/tests` are not executed in GitHub Actions CI workflows. *Remediation: Add a `coder-mcp` job to `.github/workflows/ci.yml` following the exact pattern of `news-mcp` and `artifacts-mcp` (install `./mooshik-common`, `pytest==9.1.1`, `mcp==2.1.1`, and run `pytest mcp-servers/coder/tests -q`).* |
| 2 | P3 | `README.md` | **`README.md` overview and architecture diagram omit `agy` agent.** Line 47 (`Coder["Coding Contractor (Claude / OMP / Cursor)"]`) and line 150 (`(Claude Code, Gemini CLI, or Cursor Agent)`) list only three agents, omitting `agy` (Antigravity CLI) which is supported in `mooshik configure coder --agent agy` and `coder_mcp`. *Remediation: Update lines 47 and 150 of `README.md` to mention all four supported agents (`Claude / OMP / Cursor / Antigravity` / `agy`).* |
| 3 | P3 | `dev-diary/PLAN.md` | **`dev-diary/PLAN.md` milestone table out of sync with final implementation.** Line 71 of `dev-diary/PLAN.md` describes M12g with 3 agents and "24 offline tests" (from Round 1), but the implementation expanded in Round 2 to 4 agents and 32 offline tests. *Remediation: Update `dev-diary/PLAN.md` line 71 to reflect 4 agents (`Claude Code, Gemini CLI, Cursor Agent, Antigravity CLI`) and 32 offline tests.* |

## Notes for Next Round

Remediate all 3 P3 findings:
1. Add `coder-mcp` job to `.github/workflows/ci.yml`.
2. Update `README.md` lines 47 and 150 to include `agy` / Antigravity.
3. Update `dev-diary/PLAN.md` line 71 to reference 4 agents and 32 offline tests.
