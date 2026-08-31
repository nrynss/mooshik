---
title: Artifacts MCP Server
description: Multimodal screenshot and audio ingestion using Google ADK.
---

The Artifacts MCP server extracts structured knowledge from non-text files like screenshots, whiteboard diagrams, and voice recordings.

## The `extract_artifact` Tool

The server exposes one primary tool: `extract_artifact`.

Parameters:
- `path`: Absolute or relative path to the image or audio file.
- `focus`: Optional prompt guiding the extraction.

Supported file types:
- **Images**: `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`
- **Audio**: `.wav`, `.mp3`, `.m4a`, `.ogg`

## Extraction Principles

The extractor follows strict guidelines to maintain graph quality:

1. **Typed Concepts Only**: Emits the standard concept types (`entity`, `logic`, `constraint`, `resource`, `observation`).
2. **Refuses Captions**: Never generates superficial visual captions or UI chrome descriptions.
3. **Values and Relations**: Prioritizes numerical thresholds, architectural edges, and component identities.
4. **Pre-Wire Secret Scan**: Scans extracted text for credentials and drops the entire artifact if it finds secrets.

## Server Configuration

Add the server to your Mooshik configuration:

```toml
[mcp.servers.artifacts]
command = "python3"
args = ["/path/to/mooshik/mcp-servers/artifacts/server.py"]
env = { ARTIFACTS_LOCATION = "global" }
```
