# M12f Adversarial Review: Round 1

**Verdict:** REMEDIATE

## Claims vs Reality

| Claim | Reality | Proof/Notes |
| :--- | :--- | :--- |
| All source files under `mcp-servers/artifacts/` | True | All files found in `mcp-servers/artifacts/` and well within size bounds. |
| 8 tests pass offline | True | `pytest mcp-servers/artifacts/tests -q` passes 8 tests. |
| ADK LlmAgent used with faked client | True | Uses `InMemoryRunner` and custom `InjectedGemini` subclass in `agent.py`. |
| Secret scanning with whole-document drop | **FALSE** | `backend.py` calls `find_secret(raw_response)` but entirely omits the `self.secrets` argument for vault values (`extra_forbidden`). Vault secrets will not be redacted. |
| Multimodal support (image and audio) | True | `backend.py` routes image and audio mime types appropriately to Vertex files API. |
| Tests cover secret scanning, wire protocol, and entrypoints | **FALSE** | `ArtifactsBackend` is mocked out via `ScriptedBackend`. `FakeClient` is defined but unused. Neither multimodal extraction nor secret scanning is actually tested in the backend. |

## Gate Results

- `pytest mcp-servers/artifacts/tests -q`: PASS (8 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- `cargo fmt --check`: PASS
- File size caps: PASS (Max Python file is `backend.py` at 93 lines)

## Findings

| # | Priority | File | Finding with remediation |
| :--- | :--- | :--- | :--- |
| 1 | P1 | `mcp-servers/artifacts/artifacts_mcp/backend.py` | **Secret scanner bypasses vault values.** `backend.py` calls `secret_hit = find_secret(raw_response)` but fails to pass `self.secrets` to the `extra_forbidden` parameter of `find_secret`. Vault credentials (like API keys) in the model output will not drop the document, which is a severe security hole and violates the spec. *Remediation: Pass `self.secrets` as the second argument: `find_secret(raw_response, self.secrets)`.* |
| 2 | P1 | `mcp-servers/artifacts/tests/test_artifacts.py` | **Zero test coverage for `ArtifactsBackend`.** The tests mock the entire backend out using `ScriptedBackend`. `FakeClient` and `FakeFiles` are defined in `fakes.py` but never used. There are no tests verifying the image/audio extraction path with fakes, nor tests verifying that secret scanning actually drops the artifact within the backend. *Remediation: Write unit tests in `test_artifacts.py` that instantiate `ArtifactsBackend` with `FakeClient` and verify successful extraction (image/audio) and secret-drop behavior.* |

## Notes for Next Round
Fix the secret scanning hole in `backend.py` and implement the missing unit tests for `ArtifactsBackend` in `test_artifacts.py`. Ensure the `FakeClient` is properly utilized to test the multimodal endpoints and secret scanning mechanisms.
