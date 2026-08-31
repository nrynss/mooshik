---
title: Artifacts Server
description: Multimodal concept extraction from screenshots and audio recordings via Google ADK.
---

The `artifacts` server is a stdio MCP server that extracts structured memory concepts from multimodal workspace artifacts, including screenshots and audio recordings.

## Extraction Architecture

Rather than storing raw image pixels or audio waveforms in the memory graph, the artifacts server extracts structured, typed concepts:

- **Screenshots and UI mockups:** Identifies system architecture diagrams, error dialogues, wireframe components, and data structures.
- **Audio recordings and voice memos:** Extracts meeting decisions, architecture constraints, action items, and rationale.

The extraction pipeline uses Google ADK (`google.adk.agents.LlmAgent`) powered by `gemini-3.7-flash` at Vertex AI location `global`.

## Pre-Wire Secret Scanning

Before extracted concepts cross the JSON-RPC wire to Mooshik, the server scans the text for sensitive credentials:
- Pattern checks for private keys, AWS access tokens, GitHub personal access tokens, and Slack API tokens.
- Exact match checks against known vault secret values in the process environment.
- If a secret is detected in an artifact extraction, the entire item is dropped to prevent credential leakage.

## Server Configuration

Add the server to `~/.mooshik/config.toml`:

```toml
[mcp_servers.artifacts]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-artifacts-mcp"
expose = ["extract_image_concepts", "extract_audio_concepts"]

[mcp_servers.artifacts.env]
MOOSHIK_GEMINI_PROJECT = "gemini-project"
```

Set permissions to allow artifact processing:

```toml
[permissions]
"mcp.artifacts.*" = "allow"
```

## Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `MOOSHIK_GEMINI_API_KEY` | *(unset)* | Gemini Developer API key (if using direct API auth). |
| `MOOSHIK_GEMINI_PROJECT` | *(unset)* | Vertex AI Google Cloud project ID. |
| `ARTIFACTS_LOCATION` | `global` | Vertex AI inference location. |
| `ARTIFACTS_MODEL` | `gemini-3.7-flash` | Multimodal concept extraction model. |
| `ARTIFACTS_TIMEOUT_SECS` | `45.0` | Execution timeout for extraction calls. |
| `ARTIFACTS_LOG_LEVEL` | `INFO` | Logging verbosity for stderr output. |
