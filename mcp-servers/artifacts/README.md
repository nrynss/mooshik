# Mooshik Artifacts MCP Server

Extracts typed concepts from non-text workspace artifacts (screenshots, audio recordings) using Google ADK and Gemini 3.7 Flash, returning them over stdio for Mooshik to derive into its memory graph.

## Features
* Multimodal extraction for images and audio files.
* ADK-based extraction flow via `google.adk.agents.LlmAgent`.
* Secret scanning prevents credentials from leaking to the model.

## Setup

Environment variables for configuration:
* `MOOSHIK_GEMINI_API_KEY`: Gemini Developer API key
* `MOOSHIK_GEMINI_PROJECT`: Vertex AI project ID (if not using API key)
* `ARTIFACTS_LOCATION`: Vertex AI location (defaults to `global`)
* `ARTIFACTS_MODEL`: Extraction model (defaults to `gemini-3.7-flash`)
* `ARTIFACTS_TIMEOUT_SECS`: Tool execution timeout (defaults to 45.0s)
* `ARTIFACTS_LOG_LEVEL`: Log level, e.g., `INFO`

## Run
```bash
python3 server.py
```
Or via MCP:
```json
{
  "command": "python3",
  "args": ["/path/to/mcp-servers/artifacts/server.py"]
}
```
