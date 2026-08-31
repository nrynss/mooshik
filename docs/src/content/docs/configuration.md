---
title: Configuration
description: Configure stores, embedders, companions, and environment variables.
---

Mooshik reads its configuration from `~/.mooshik/config.toml` and applies environment variable overrides on startup.

## Configuration File Format

Here is an example `config.toml` file with local and shared options:

```toml
[session]
id = "workspace-default"
agent = "mooshik"

[store]
kind = "sqlite"
path = "~/.mooshik/graph.db"

[embedder]
kind = "bge_m3"
endpoint = "http://127.0.0.1:8080/v1"
dim = 1024

[companion]
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
max_tokens = 4096
temperature = 0.2
```

## Storage Options

Mooshik supports two storage backends:

1. **SQLite (`sqlite`)**: Stores graph nodes locally in a single SQLite database file. No external database server is required.
2. **Postgres (`postgres`)**: Connects to a PostgreSQL database for shared multi-machine workspaces. Requires a valid database connection string.

## Embedder Options

The embedder turns text into vector representations:

1. **BGE-M3 (`bge_m3`)**: Connects to a local embedding server such as llama.cpp.
2. **Gemini (`gemini`)**: Uses Google Vertex AI embeddings. Requires service account credentials or an API key.

## Environment Variable Overlays

You can override settings at runtime using environment variables:

- `MOOSHIK_POSTGRES_DSN`: Overrides the Postgres database connection string.
- `MOOSHIK_COMPANION_BASE_URL`: Overrides the companion endpoint URL.
- `MOOSHIK_COMPANION_MODEL`: Overrides the companion model name.
- `MOOSHIK_GEMINI_CREDENTIALS`: Path to Google Cloud service account JSON file.

## Updating Settings via CLI

You can inspect and update settings safely without editing the TOML file directly:

```sh
mooshik config show
mooshik config set companion.model "google/gemini-3.7-flash"
```
