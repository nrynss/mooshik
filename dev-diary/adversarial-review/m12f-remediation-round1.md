# M12f Remediation Report: Round 1

## Findings Addressed

### Finding 1
**Secret scanner bypasses vault values.**
- **Changes**: Modified `mcp-servers/artifacts/artifacts_mcp/backend.py` to pass `self.secrets` as the second argument to `find_secret`. It now correctly drops the document if vault secrets are found in the model output.
- **Additional fixes**: Found and fixed two bugs preventing `ArtifactsBackend` from correctly running extraction via ADK. Added `runner.auto_create_session = True` to the `InMemoryRunner`, updated keyword arguments to `types.Part.from_text(text=...)`, and corrected event parsing logic to handle `InMemoryRunner` model output correctly.

### Finding 2
**Zero test coverage for `ArtifactsBackend`.**
- **Changes**: Added 6 tests to `mcp-servers/artifacts/tests/test_artifacts.py` using `ArtifactsBackend` and `FakeClient`:
  - `test_extract_image_success`
  - `test_extract_audio_success`
  - `test_extract_drops_on_secret`
  - `test_extract_drops_on_vault_value`
  - `test_extract_missing_file`
  - `test_extract_unsupported_file`
- **Changes**: Augmented `FakeClient`, `FakeModels`, and `FakeResponse` in `mcp-servers/artifacts/tests/fakes.py` to correctly mimic the `GenerateContentResponse` required by the ADK framework. Added `aio` mocking, `usage_metadata`, and mocked dummy metadata fields to ensure tests correctly execute through the model runner.

## Gate Results
- `pytest mcp-servers/artifacts/tests -q`: PASS (14 passed)
- `pytest mooshik-common/tests -q`: PASS (15 passed)
- `pytest mcp-servers/news/tests -q`: PASS (53 passed)
- File size caps: PASS (Max Python file is well within size bounds)
- Verified no stray stdout prints.

## New Concerns Discovered
During remediation of Finding 2, several issues with `ArtifactsBackend` were identified that would have crashed the server in production, but were hidden previously due to only being tested against `ScriptedBackend`. These included `TypeError` from missing keywords in `from_text`, uncreated sessions in `InMemoryRunner`, and invalid event checks. All were patched.
