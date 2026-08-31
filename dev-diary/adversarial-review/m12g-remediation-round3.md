# M12g Remediation: Round 3

## Fixes Applied

| # | Review Finding | File | Change Made |
| :--- | :--- | :--- | :--- |
| 1 | Missing `coder-mcp` job in `.github/workflows/ci.yml` | [`.github/workflows/ci.yml`](file:///home/nryn/work/mooshik/.github/workflows/ci.yml) | Added `coder-mcp` job following the exact pattern of `news-mcp` and `artifacts-mcp` (installs `./mooshik-common`, `pytest==9.1.1`, `mcp==2.1.1`, and runs `pytest mcp-servers/coder/tests -q`). |
| 2 | `README.md` diagram and overview omit `agy` / Antigravity | [`README.md`](file:///home/nryn/work/mooshik/README.md) | Updated architecture diagram node label to `Coder["Coding Contractor (Claude / OMP / Cursor / Antigravity)"]` and updated overview text to mention all 4 agents (`Claude Code, Gemini CLI, Cursor Agent, or Antigravity / agy`). |
| 3 | `dev-diary/PLAN.md` line 71 summary out of sync | [`dev-diary/PLAN.md`](file:///home/nryn/work/mooshik/dev-diary/PLAN.md) | Updated the M12g summary row in `dev-diary/PLAN.md` to record 4 agents (`Claude Code, Gemini CLI, Cursor Agent, Antigravity CLI`) and 32 offline tests. |

## Gate Results After Remediation

- `pytest mcp-servers/coder/tests -q`: PASS (32 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `cargo test`: PASS (636 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (All `.rs` files <= 1500 lines)

## Summary

3 findings remediated. All gate tests pass. Zero residue.
