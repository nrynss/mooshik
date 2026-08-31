---
title: CLI Reference
description: Complete command-line interface reference for all Mooshik commands and flags.
---

Mooshik provides a comprehensive command-line interface for terminal interaction, memory management, configuration, and secret management.

## Commands Overview

### `mooshik init`

Launches the interactive first-run setup wizard.

```bash
mooshik init [--non-interactive]
```

- `--non-interactive`: Skips terminal prompts and writes default configuration directly.

### `mooshik tui`

Launches the interactive terminal pane.

```bash
mooshik tui [--demo [scene]]
```

- `--demo`: Opens standalone interface artboards without opening a database. Supported values: `today`, `recall`, `caution`.

### `mooshik chat`

Starts an interactive conversational session directly in the terminal without opening the TUI.

```bash
mooshik chat
```

### `mooshik recall`

Searches the memory graph for concepts and architectural decisions matching a text query.

```bash
mooshik recall "<query>"
```

### `mooshik stats`

Displays graph node counts, edge counts, canonical fact metrics, and write-behind flush status.

```bash
mooshik stats
```

### `mooshik reflect`

Runs a memory consolidation pass, merging paraphrase twins and synthesizing daily prose summaries.

```bash
mooshik reflect [--dry-run]
```

- `--dry-run`: Reports planned merges and generated prose without modifying the database.

### `mooshik serve`

Serves Lambo's MCP memory surface on stdio and publishes a local session endpoint so other processes can proxy into memory.

```bash
mooshik serve
```

### `mooshik config show`

Displays the active configuration with sensitive values redacted.

```bash
mooshik config show
```

### `mooshik config set`

Updates a configuration setting safely.

```bash
mooshik config set <key> <value> [--confirm-database-change]
```

- `--confirm-database-change`: Required when changing `store.kind` to confirm migrating the active database.

### `mooshik configure coder`

Configures the coding contractor MCP server block in `config.toml`.

```bash
mooshik configure coder --agent <claude|omp|cursor|agy>
```

- `--agent <name>`: Target coding agent CLI (`claude`, `omp`, `cursor`, or `agy`).

### `mooshik permissions`

Lists all active tool permission decisions and grants.

```bash
mooshik permissions
```

### `mooshik secret set`

Stores a secret in the encrypted local vault with terminal echo disabled.

```bash
mooshik secret set <name>
```

### `mooshik secret get`

Retrieves and prints a decrypted secret value.

```bash
mooshik secret get <name>
```

### `mooshik secret list`

Lists the names of all registered secrets in the local vault.

```bash
mooshik secret list
```

## Exit Codes

| Exit Code | Meaning | Example Scenario |
| :--- | :--- | :--- |
| `0` | Success | Normal command completion. |
| `1` | Runtime Failure | Network timeout or tool execution error. |
| `2` | Configuration or Conflict | Missing database DSN, invalid arguments, or database session lease collision. |
