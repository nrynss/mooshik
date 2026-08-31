# M12f Remediation Report: Round 2

## Findings Addressed

**Finding 1 (P1): Missing explicit refusals in extraction prompt.**
- Updated `PROMPT` in `mcp-servers/artifacts/artifacts_mcp/agent.py` to explicitly include refusal rules for descriptions of the artifact as an artifact, UI chrome, OCR dumps of everything visible, and anything that is not a claim about the workspace.

**Finding 2 (P3): Untracked source files.**
- Added all files in `mcp-servers/artifacts/` to git tracking.
- Confirmed that `__pycache__/` and `.pytest_cache/` are in the root `.gitignore` and not tracked by git.

## Gate Results
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- File size caps: PASS
- Verified no stray stdout prints.

## New Concerns Discovered
None.
