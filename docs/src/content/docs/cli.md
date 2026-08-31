---
title: CLI Command Reference
description: Complete reference for all Mooshik command-line interface commands.
---

This page provides the syntax and options for all Mooshik CLI commands.

## `mooshik init`

Initializes the local Mooshik directory layout and configuration.

```sh
mooshik init
```

## `mooshik tui`

Launches the interactive terminal user interface.

```sh
mooshik tui [--demo [scene]]
```

Flags:
- `--demo`: Opens design artboards without connecting to a database.

## `mooshik chat`

Starts an interactive terminal chat session with the companion model.

```sh
mooshik chat
```

## `mooshik recall`

Searches the memory graph for relevant concepts.

```sh
mooshik recall <query>
```

## `mooshik stats`

Prints concept counts and health statistics for the active session.

```sh
mooshik stats
```

## `mooshik reflect`

Runs graph consolidation and generates prose summaries.

```sh
mooshik reflect [--dry-run]
```

Flags:
- `--dry-run`: Reports planned merges without applying database changes.

## `mooshik config`

Manages configuration keys.

```sh
mooshik config show
mooshik config set <key> <value> [--confirm-database-change]
```

## `mooshik configure coder`

Configures the coding contractor MCP server block and vault secrets.

```sh
mooshik configure coder --agent <name>
```

Options:
- `--agent <name>`: Coding agent to delegate to (`claude`, `omp`, `cursor`, `agy`).

## `mooshik secret`

Stores and manages encrypted secrets in the local vault.

```sh
mooshik secret set <name>
```

## `mooshik permissions`

Manages tool execution grants.

```sh
mooshik permissions
```
