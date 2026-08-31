---
title: Configuration Schema
description: Detailed schema of all keys and tables in config.toml.
---

Mooshik organizes settings into top-level TOML tables in `~/.mooshik/config.toml`.

## `[session]` Table

Defines workspace identity.

| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `id` | String | `default` | Unique name of the memory session. |
| `agent` | String | `mooshik` | Agent identity tag. |

## `[store]` Table

Configures graph storage.

| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `kind` | String | `sqlite` | Storage backend (`sqlite`, `postgres`, `memory`). |
| `path` | String | `~/.mooshik/graph.db` | File path for SQLite. |
| `dsn` | String | None | Connection string for Postgres. |
| `dsn_secret` | String | None | Vault secret key containing Postgres DSN. |

## `[embedder]` Table

Configures vector embeddings.

| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `kind` | String | `bge_m3` | Embedder type (`bge_m3`, `gemini`, `fixture`). |
| `endpoint` | String | None | HTTP endpoint for local models. |
| `dim` | Integer | `1024` | Vector dimension size. |

## `[companion]` Table

Configures the companion chat model.

| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `base_url` | String | `http://127.0.0.1:8080/v1` | OpenAI-compatible endpoint. |
| `model` | String | `local-model` | Model name. |
| `max_tokens` | Integer | `4096` | Maximum token limit. |
| `temperature` | Float | `0.2` | Sampling temperature. |
