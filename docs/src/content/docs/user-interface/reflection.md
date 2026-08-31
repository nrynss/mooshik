---
title: Memory Reflection
description: Consolidate duplicate concepts and generate prose summaries.
---

The reflection engine cleans the concept graph and writes concise prose summaries for the terminal UI.

## What Reflection Does

Over time, similar ideas enter the graph under slightly different names. Reflection runs periodically to consolidate these variations.

### 1. Paraphrase Merging
The reflector compares concept embeddings using cosine similarity. When two nodes exceed the similarity threshold, the engine merges them into a single primary concept. It redirects all edges to the survivor node and preserves original details.

### 2. Daily Mood & Notes
Reflection generates a high-level mood and concise notes for each active day.

### 3. Thread Reasons
It infers the underlying technical motivation for ongoing threads, answering why a decision occurred.

## Running Reflection

Execute a reflection pass across your active graph:

```sh
mooshik reflect
```

### Previewing Changes (Dry Run)

You can preview proposed concept merges and prose summaries without writing to the database:

```sh
mooshik reflect --dry-run
```

The dry run reports planned cluster merges, survivor selections, and summary text directly to your terminal.
