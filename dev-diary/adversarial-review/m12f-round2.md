# M12f Adversarial Review: Round 2

**Verdict:** REMEDIATE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| Fixed `backend.py` to pass `self.secrets` to `find_secret` | True | `backend.py` now correctly invokes `find_secret(raw_response, self.secrets)`. |
| Added 6 new backend unit tests | True | `test_artifacts.py` now actually uses `ArtifactsBackend` with fakes and passes tests for audio, image, missing file, and secrets. |
| All gates passing | True | `pytest mcp-servers/artifacts/tests -q` passes 14 tests. Other gates pass as well. |
| Fixed runtime bugs in `ArtifactsBackend` | True | Verified that `types.Part.from_text` usage, ` InMemoryRunner` session logic, and model event output logic were corrected. |

## Gate Results

- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (Max Python file is `test_artifacts.py` at 142 lines)

## Findings

| # | Priority | File | Finding with remediation |
| :--- | :--- | :--- | :--- |
| 1 | P1 | `mcp-servers/artifacts/artifacts_mcp/agent.py` | **Missing explicit refusals in extraction prompt.** The spec (`PLAN.md` lines 884-887) mandates that the prompt must explicitly refuse: descriptions of the artifact as an artifact, UI chrome (window titles, button labels, browser tabs, menu bars), OCR dumps of everything visible, and anything that is not a claim about the workspace. The current prompt only says "never captions" but misses all these other required refusals, risking polluted memory graphs. *Remediation: Update `PROMPT` in `agent.py` to explicitly include these rules.* |
| 2 | P3 | `mcp-servers/artifacts/` | **Untracked source files.** The `mcp-servers/artifacts` directory has many untracked files instead of being properly added and committed in Git, likely from the first remediation round. *Remediation: Ensure files are added and committed before the milestone concludes.* |

## Notes for Next Round
The missing prompt instructions from the spec represent a major gap that undermines the sparse graph guarantees of the memory system. Once the prompt is updated to refuse UI chrome and OCR dumps, the implementation should be solid. Tests and backend runtime fixes look good.
