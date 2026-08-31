# Milestone M12f Implementation Report

## Architecture
The `artifacts_mcp` server follows the pattern established in `news_mcp`. It extracts memory concepts from non-text artifacts (images, audio).

### Key Decisions
1. **Tool Output & Write Avoidance**: The server strictly returns typed JSON concepts (`entity`, `logic`, `constraint`, `resource`, `observation`) and *does not* write to the graph. This adheres to the rule that writing is Mooshik's job (`lambo_derive`).
2. **Secret Scanning**: Scans raw model outputs before returning them over the wire to ensure `vault-value` or matched patterns drop the whole document (concept-level scanning as required).
3. **ADK Usage**: `google.adk.agents.LlmAgent` is utilized to wrap the instructions. We subclass `Gemini` via `InjectedGemini` to inject the constructed `google.genai.Client` that respects offline fakes and credentials without invoking ADK's internal auth pathways.
4. **Multimodal**: Uses Vertex AI / Gemini Developer API via `files.upload` before model inference.

## Quality Gates
- **Tests**: Passed all offline tests covering configs, secret scanning, wire protocol, and entrypoints.
- **CI**: Added `artifacts-mcp` job pinned to the required commit SHAs.
- **Files**: All within expected size caps.
