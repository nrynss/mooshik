---
title: Configuration Schema
description: Full reference of all settable keys, TOML tables, and validation types.
---

This page documents the complete configuration schema for `~/.mooshik/config.toml` and every key supported by `mooshik config set`.

## Settable Keys Reference

The following 19 keys can be updated using `mooshik config set <key> <value>`:

| Key | Expected Type | Description |
| :--- | :--- | :--- |
| `store.kind` | `sqlite` \| `postgres` | Graph storage backend. |
| `store.dsn_secret` | Secret Name | Vault secret name holding PostgreSQL connection DSN. |
| `store.path` | File Path | SQLite database file path. |
| `embedder.kind` | `fixture` \| `bge_m3` \| `gemini` | Embedding provider. |
| `embedder.dim` | Positive Integer | Vector embedding dimension (e.g. `1024` or `1536`). |
| `embedder.gemini_project` | String | Google Cloud project ID for embeddings. |
| `embedder.gemini_location` | String | Google Cloud region for embeddings (`us-central1`). |
| `embedder.gemini_model` | String | Embedding model name (`gemini-embedding-001`). |
| `embedder.gemini_credentials` | File Path | Path to service account JSON for embeddings. |
| `daemon.flush_interval_ms` | Positive Integer | Background flush interval in milliseconds. |
| `companion.base_url` | URL | Endpoint URL for OpenAI-compatible `/v1` companion. |
| `companion.model` | String | Language model identifier. On the Google posture this needs the publisher prefix: `google/gemini-3.7-flash`. |
| `companion.api_key_secret` | Secret Name | Vault secret name holding companion API bearer key. |
| `companion.auth` | `none` \| `bearer` \| `google` | Authentication method for the companion endpoint. |
| `companion.google_project` | String | Google Cloud project ID for Vertex AI companion. |
| `companion.google_location` | String | Google Cloud location for Vertex AI (`global`). |
| `companion.google_credentials` | File Path | Path to service account JSON for Vertex AI companion. |
| `companion.context_window` | Positive Integer | Maximum context window tokens. |
| `companion.temperature` | Number | Sampling temperature for model completions. |

## TOML Tables Structure

### `[vault]`

Configures the local secret vault encryption provider.

```toml
[vault]
provider = "keyring"   # "keyring" or "passphrase"
```

### `[store]`

Defines graph storage persistence.

```toml
[store]
kind = "postgres"
dsn_secret = "store-dsn"
```

### `[embedder]`

Defines vector generation parameters.

```toml
[embedder]
kind = "gemini"
dim = 1536
gemini_location = "us-central1"
gemini_model = "gemini-embedding-001"
```

### `[companion]`

Configures the language model for conversational turns and tool execution.

```toml
[companion]
auth = "google"
google_project = "my-project"
google_location = "global"
model = "google/gemini-3.7-flash"
context_window = 32768
temperature = 0.2
```

### `[permissions]`

Controls tool execution policies.

```toml
[permissions]
memory = ["recall", "derive"]
scratch = "prompt"
"mcp.news.*" = "allow"
"mcp.coder.*" = "prompt"
```

### `[mcp_servers.<name>]`

Configures external stdio Model Context Protocol servers.

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
MOOSHIK_GEMINI_PROJECT = "gemini-project"
```
