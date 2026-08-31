---
title: Chat and Recall
description: Use CLI subcommands for command-line conversations, memory search, and session statistics.
---

While the terminal pane (`mooshik tui`) is the primary interface, Mooshik provides secondary CLI commands for quick terminal queries, memory inspection, and scripting.

## Command-Line Chat (`mooshik chat`)

Run a quick conversation session directly in your shell:

```bash
mooshik chat
```

`mooshik chat` connects to your configured companion model, loads current workspace memory, and provides interactive turn-based responses.

Tool permissions apply normally: memory tools are allowed, scratch scripts prompt for confirmation, and all other tools require explicit grants.

## Searching Memory (`mooshik recall`)

Query the knowledge graph for stored architecture decisions, project conventions, and operational constraints:

```bash
mooshik recall "deployment checklist"
```

### Understanding Search Output

`mooshik recall` returns ranked concepts matching your query:

```text
Matches for 'deployment checklist':
- [canonical] release pipeline requires green CI checks (relevance 0.89, blast radius 7)
- database migrations must be applied before deploying workers (relevance 0.76, blast radius 3)
```

- **Canonical badge:** Indicates the concept earned permanent status through recurrence across sessions.
- **Relevance:** Combined dense embedding and keyword matching score.
- **Blast radius:** Number of dependent concepts, files, and actions connected to this fact.

> [!NOTE]
> Recall output is printed directly to stdout for the local operator. It is not passed through egress redaction because no external model sees the command output.

## Graph Health Metrics (`mooshik stats`)

Display graph size and persistence metrics:

```bash
mooshik stats
```

The output summarizes session health:
- Total node count (concepts, resources, and interactions).
- Total edge count (structural dependencies and associations).
- Number of promoted canonical facts.
- Write-behind durability status (unflushed queue depth and flush lag).
