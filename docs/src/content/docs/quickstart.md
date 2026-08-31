---
title: Quickstart Guide
description: Initialize your workspace and run your first Mooshik session.
---

This guide walks you through setting up and running Mooshik in your workspace.

## Step 1: Initialize Workspace

Run the initialization command inside your workspace:

```sh
mooshik init
```

This command creates your private configuration directory at `~/.mooshik/` with safe file permissions. It writes a default `config.toml` file and provisions an encrypted local vault.

## Step 2: Configure Backends

Mooshik supports local and shared backends. By default, it provisions a local SQLite database for zero-configuration startup.

Verify your configuration settings:

```sh
mooshik config show
```

## Step 3: Launch the Terminal UI

Open the interactive terminal user interface:

```sh
mooshik tui
```

The interface shows your current weekly timeline, active threads, and memory concepts. It rebuilds automatically whenever the underlying graph updates.

## Step 4: Recall Memory Concepts

Query your memory graph from the command line:

```sh
mooshik recall "database migration steps"
```

Mooshik searches the concept graph and returns the most relevant context and relations.

## Step 5: Consolidate Memories

Run a reflection pass to clean duplicate concepts and write human-readable prose summaries:

```sh
mooshik reflect
```

To preview the changes without modifying the database, add the dry run flag:

```sh
mooshik reflect --dry-run
```
