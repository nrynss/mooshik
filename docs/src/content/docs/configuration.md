---
title: Configuration
description: Manage configuration files, encrypted vault secrets, and environment overlays.
---

Mooshik stores its configuration in `~/.mooshik/config.toml`. It manages credentials separately through an encrypted local vault.

## Durable Configuration Management

Always configure settings using the CLI. The CLI validates types and updates `config.toml` safely.

Inspect your active configuration with redacted secrets:

```bash
mooshik config show
```

Update a setting:

```bash
mooshik config set companion.model "gemini-3.7-flash"
```

### Storing Credentials in the Vault

Never write credentials, passwords, or connection strings into `config.toml`.

Store the secret in the encrypted vault:

```bash
mooshik secret set store-dsn
```

Link the vault secret in your configuration:

```bash
mooshik config set store.dsn_secret "store-dsn"
```

This ensures credentials remain encrypted at rest and never leak into git repositories, shell history, or process tables.

## Environment Variable Escape Hatches

Mooshik supports environment variables as non-durable escape hatches. Use them for temporary testing or containerized deployments.

| Variable | Target Key | Notes |
| :--- | :--- | :--- |
| `MOOSHIK_HOME` | Home directory | Overrides `~/.mooshik` path. |
| `MOOSHIK_POSTGRES_DSN` | `store.dsn` | Escape hatch for database DSN. Does not persist across reboots. |
| `MOOSHIK_COMPANION_MODEL` | `companion.model` | Overrides companion model identifier. |
| `MOOSHIK_COMPANION_BASE_URL` | `companion.base_url` | Overrides OpenAI-compatible endpoint URL. |
| `MOOSHIK_GEMINI_PROJECT` | `embedder.gemini_project` | Overrides Google Cloud project ID. |
| `MOOSHIK_GEMINI_LOCATION` | `embedder.gemini_location` | Names the **embedder** region (`us-central1`). |
| `MOOSHIK_GEMINI_CREDENTIALS` | `embedder.gemini_credentials` | Path to Google Cloud service account JSON file. |

> [!WARNING]
> Environment variables do not survive reboots or session restarts. For persistent operation, store connection strings with `mooshik secret set` and set `store.dsn_secret`.

## The Vertex Location Rule

In shared posture deployments, inference and embedding locations must differ:

- `companion.google_location = "global"`: Vertex AI serves Gemini 3.x Flash models from `global` only.
- `embedder.gemini_location = "us-central1"`: The `gemini-embedding-001` model lives in `us-central1`.

Setting both options to the same region causes connection failures.

## Permissions Block

Control tool execution boundaries under the `[permissions]` table:

```toml
[permissions]
memory = ["recall", "derive"]
scratch = "prompt"
"mcp.news.*" = "allow"
"mcp.coder.*" = "prompt"
```

- `"allow"`: Executes without asking.
- `"prompt"`: Prompts for confirmation in the terminal before running.
- `"deny"`: Refuses execution.

## MCP Server Blocks

Configure external Model Context Protocol servers under `[mcp_servers.<name>]`:

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
MOOSHIK_GEMINI_API_KEY = "gemini-api-key"
```

Rules for MCP server blocks:
1. `expose` is an allowlist. An empty list leaves the server disabled.
2. Values in `[mcp_servers.<name>.env]` are vault secret names, not literal tokens. Mooshik resolves them at launch from the local vault.
