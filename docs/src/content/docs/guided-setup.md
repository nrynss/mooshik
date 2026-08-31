---
title: Guided Setup
description: Step-by-step walkthrough of the interactive mooshik init wizard.
---

`mooshik init` is an interactive setup flow that configures your storage, embedder, and companion models.

Run the wizard from your terminal:

```bash
mooshik init
```

## Design Principles

- **One question at a time.** The wizard prompts for one setting at a time and writes the answer immediately using the same verified writer as `mooshik config set`.
- **Zero-echo secret entry.** Passwords, connection strings, and API keys are read with terminal echo disabled. Secrets go directly into the encrypted vault at `~/.mooshik/vault`. They never touch `config.toml`, shell history, or process listings.
- **Interactive TTY detection.** The wizard prompts only on a real terminal. When run without a TTY or with `--non-interactive`, it writes default values without prompting.
- **Resumable and safe to rerun.** Rerunning `mooshik init` asks only for unset values and confirms existing configuration without overwriting working settings.
- **Immediate verification.** The wizard verifies each connection. It provisions the database schema, tests the embedder with a probe string, and tests companion inference. If a check fails, it offers a retry.

## Step-by-Step Flow

### 1. Choosing a Posture

The wizard asks which deployment posture you want:
- **Shared (default):** Uses PostgreSQL with pgvector and Google Cloud Vertex AI models. Enables shared memory across multiple machines.
- **Local:** Uses SQLite and a local OpenAI-compatible endpoint. All data remains on your machine.

### 2. Shared Posture Configuration

When you select the shared posture, the wizard collects the following settings:

1. **Database connection string (DSN):** Read without echo into the vault under the name `store-dsn`. The wizard sets `store.dsn_secret = "store-dsn"`, `store.kind = "postgres"`, and provisions the graph schema.
2. **Google Cloud project:** Sets both `embedder.gemini_project` and `companion.google_project`. You can provide different project IDs if inference and embedding live in separate projects.
3. **Google credentials path:** Sets both `companion.google_credentials` and `embedder.gemini_credentials`.
4. **Derived defaults:** The wizard sets `companion.auth = "google"`, `companion.google_location = "global"`, and `companion.model = "gemini-3.7-flash"`.

> [!NOTE]
> Inference runs at `global` because Vertex AI serves Gemini 3.x Flash models from `global` only. Embedding runs at `us-central1` because `gemini-embedding-001` lives in that region.

### 3. Local Posture Configuration

When you select the local posture, the wizard configures local stores:

1. **Graph storage path:** Sets `store.kind = "sqlite"` and prompts for the database file path (default `~/.mooshik/mooshik.db`).
2. **Local embedder:** Sets `embedder.kind = "bge_m3"` and dimension `1024`.
3. **Companion endpoint:** Prompts for your OpenAI-compatible endpoint URL (`companion.base_url`), model identifier (`companion.model`), and optional API key secret name (`companion.api_key_secret`).

> [!IMPORTANT]
> The embedder contract is sticky. Changing embedder kind, model, or vector dimension later invalidates existing vector indices and requires re-indexing.

### 4. Wiring MCP Servers

If the wizard detects the virtualenv created by the installer at `~/.local/share/mooshik/venv`, it offers to configure the Python MCP servers:
- **News server (`news`):** Provides live web search and article fetching grounded in Google Search.
- **Artifacts server (`artifacts`):** Extracts structured concepts from screenshots and audio recordings.
- **Coder server (`coder`):** Delegates file modifications to external coding agents (`claude`, `omp`, `cursor`, or `agy`).

If you enable the coder server, the wizard prompts for your target agent and sets up the server block.

## Completing the Setup

On a fresh install, the memory graph starts empty. It fills ambiently as you work in the terminal pane.

The wizard finishes by telling you where to launch the interface:

```bash
cd ~/work
mooshik tui
```
