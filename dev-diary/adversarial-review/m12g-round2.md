# M12g Adversarial Review: Round 2

**Verdict:** REMEDIATE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| All 8 Round 1 findings remediated | True | Verified in `coder_mcp/config.py`, `backend.py`, `configure.rs`, and `test_coder.py`. |
| Non-blocking fire-and-forget `delegate` (<60s) | True | `backend.py:114-142` spawns subprocess via `subprocess.Popen` and returns JSON handle immediately. |
| Daemon boundary holds on Linux | True | `backend.py:39-45, 117-122` isolates stdio (`stdin=subprocess.DEVNULL`) and sets `PR_SET_PDEATHSIG = SIGTERM` via `preexec_fn`. |
| Permission grant is `prompt`, never `allow` | True | `src/cli/configure.rs:207, 211` sets `"mcp.coder.*" = "prompt"`. |
| Secrets handled via vault refs in config | True | `src/cli/configure.rs:216-218` sets secret names in `[mcp_servers.coder.env]`; `MOOSHIK_SECRET_VALUE` populated into vault without stdin blocking. |
| Standing rule `AGENTS.md` written before spawn | True | `backend.py:97` invokes `write_standing_rule(repo)` prior to `subprocess.Popen`. |
| `check` status reporting (running/exited/unknown) | True | `backend.py:153-177` checks bounded `_exited` map, active `_handles`, and process poll state. |
| Support for all 4 agents (`claude`, `omp`, `cursor`, `agy`) | True | `VALID_AGENTS` in `config.py:32`, `build_agent_command` in `agents.py:73-80`, and clap parser in `command.rs:154`. |
| Coder MCP package documentation `README.md` exists | **FALSE** | `mcp-servers/coder/README.md` is missing despite being explicitly referenced in `coder_mcp/__main__.py:35`. |
| CLI documentation reflects actual command signatures | **FALSE** | `README.md:178` and `docs/src/content/docs/cli.md:76` list `mooshik secret set <name> <val>` (secrets are never passed via CLI args). `cli.md:84` and `mcp-host.md:38` list `mooshik permissions list` (subcommand does not exist). |
| Docstring and comment accuracy across `coder_mcp` | **FALSE** | Multiple docstrings and `SERVER_INSTRUCTIONS` still refer to 3 agents and omit `agy` (`__init__.py:4`, `config.py:15`, `agents.py:1`, `backend.py:54`, `tools.py:39`). |

## Gate Results

- `pytest mcp-servers/coder/tests -q`: PASS (32 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (Max Python file is `test_coder.py` at 426 lines)

## Findings

| # | Priority | File | Finding with remediation |
| :--- | :--- | :--- | :--- |
| 1 | P3 | `mcp-servers/coder/README.md`, `mcp-servers/coder/coder_mcp/__main__.py` | **Missing `mcp-servers/coder/README.md`.** `coder_mcp/__main__.py:35` instructs operators: `"see mcp-servers/coder/README.md"`, but this file was omitted when the server was created. `news` and `artifacts` servers both provide complete READMEs documenting tools, configuration, environment variables, and standalone invocation. *Remediation: Create `mcp-servers/coder/README.md` documenting the coder server, its 2 tools (`delegate`, `check`), supported agents (`claude`, `omp`, `cursor`, `agy`), configuration via `~/.mooshik/config.toml`, and environment variables.* |
| 2 | P3 | `README.md`, `docs/src/content/docs/cli.md` | **`mooshik secret set` documented with invalid `<val>` argument.** `README.md:178` and `docs/src/content/docs/cli.md:76` document `mooshik secret set <name> <val>` / `<value>`. `mooshik secret set` does not accept plaintext values on the command line to prevent credential exposure in `ps` and shell history; it reads from stdin or `MOOSHIK_SECRET_VALUE`. *Remediation: Update `README.md` line 178 to `mooshik secret set <name>` and `docs/src/content/docs/cli.md` line 76 to `mooshik secret set <name>`.* |
| 3 | P3 | `docs/src/content/docs/cli.md`, `docs/src/content/docs/mcp-host.md` | **Documentation references non-existent `mooshik permissions list` and omits `configure coder`.** `docs/src/content/docs/cli.md:84` and `docs/src/content/docs/mcp-host.md:38` reference `mooshik permissions list`, which fails with exit code 2. Furthermore, `docs/src/content/docs/cli.md` omits `mooshik configure coder --agent <name>`. *Remediation: Change `mooshik permissions list` to `mooshik permissions` in `cli.md` and `mcp-host.md`. Add `## mooshik configure coder` section to `cli.md`.* |
| 4 | P3 | `mcp-servers/coder/coder_mcp/__init__.py`, `config.py`, `agents.py`, `backend.py`, `tools.py` | **Docstrings and server instructions omit `agy` agent.** When `agy` was added as a supported agent option in Round 2, several docstrings were left mentioning only 3 agents: `__init__.py:4` (`Claude Code, Gemini CLI / OMP, Cursor Agent CLI`), `config.py:15` (`The agent choice — claude, omp, or cursor`), `agents.py:1` (`for the three supported coding agents`), `backend.py:54` (`("claude", "omp", "cursor")`), and `tools.py:39` (`Claude Code, Gemini CLI, or Cursor Agent`). *Remediation: Update docstrings and `SERVER_INSTRUCTIONS` in `__init__.py`, `config.py`, `agents.py`, `backend.py`, and `tools.py` to mention all four supported agents (`claude`, `omp`, `cursor`, `agy` / Antigravity).* |

## Notes for Next Round

Remediate all 4 P3 findings:
1. Create `mcp-servers/coder/README.md`.
2. Fix `mooshik secret set <name>` in `README.md` and `docs/src/content/docs/cli.md`.
3. Fix `mooshik permissions` and add `mooshik configure coder` in `docs/src/content/docs/cli.md` and `docs/src/content/docs/mcp-host.md`.
4. Update docstrings and `SERVER_INSTRUCTIONS` in `__init__.py`, `config.py`, `agents.py`, `backend.py`, and `tools.py` to include `agy`.
