# M12g Remediation: Round 2

## Fixes Applied

| # | Review Finding | File | Change Made |
| :--- | :--- | :--- | :--- |
| 1 | Missing `mcp-servers/coder/README.md` | [`mcp-servers/coder/README.md`](file:///home/nryn/work/mooshik/mcp-servers/coder/README.md) | Created comprehensive README documenting server capabilities, the 2 tools (`delegate`, `check`), the 4 supported agents (`claude`, `omp`, `cursor`, `agy`), configuration in `~/.mooshik/config.toml`, CLI helper `mooshik configure coder`, permission grants (`"mcp.coder.*" = "prompt"`), environment variables, standing rule generation (`AGENTS.md`), daemon lifecycle boundary, and test/execution instructions. |
| 2 | `mooshik secret set` documented with invalid `<val>` argument | [`README.md`](file:///home/nryn/work/mooshik/README.md), [`docs/src/content/docs/cli.md`](file:///home/nryn/work/mooshik/docs/src/content/docs/cli.md) | Updated `mooshik secret set <name> <val>` in `README.md` and `mooshik secret set <name> <value>` in `cli.md` to `mooshik secret set <name>`. Secrets are ingested via stdin or `MOOSHIK_SECRET_VALUE`, never passed via plaintext CLI arguments. |
| 3 | Documentation references non-existent `mooshik permissions list` and omits `configure coder` | [`docs/src/content/docs/cli.md`](file:///home/nryn/work/mooshik/docs/src/content/docs/cli.md), [`docs/src/content/docs/mcp-host.md`](file:///home/nryn/work/mooshik/docs/src/content/docs/mcp-host.md) | Corrected `mooshik permissions list` to `mooshik permissions` in `cli.md` and `mcp-host.md`. Added full `## mooshik configure coder` reference section to `cli.md`. |
| 4 | Docstrings and server instructions omit `agy` agent | [`mcp-servers/coder/coder_mcp/__init__.py`](file:///home/nryn/work/mooshik/mcp-servers/coder/coder_mcp/__init__.py), [`config.py`](file:///home/nryn/work/mooshik/mcp-servers/coder/coder_mcp/config.py), [`agents.py`](file:///home/nryn/work/mooshik/mcp-servers/coder/coder_mcp/agents.py), [`backend.py`](file:///home/nryn/work/mooshik/mcp-servers/coder/coder_mcp/backend.py), [`tools.py`](file:///home/nryn/work/mooshik/mcp-servers/coder/coder_mcp/tools.py) | Updated module docstrings, type comments, and `SERVER_INSTRUCTIONS` across all package files to explicitly mention all 4 supported agents (`claude`, `omp`, `cursor`, `agy`). Updated doc comments in `src/cli/configure.rs` and `pyproject.toml` as well. |

## Gate Results After Remediation

- `pytest mcp-servers/coder/tests -q`: PASS (32 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS

## Summary

All 4 P3 findings remediated. Zero residue. All test suites and gate checks pass.
