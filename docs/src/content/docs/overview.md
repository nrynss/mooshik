---
title: Product Overview
description: Understand Mooshik's design philosophy and the triad model.
---

Mooshik is an ambient AI cowork partner and workspace orchestrator. It runs continuously alongside you as a peer.

Traditional coding agents act like contractors. You summon them, they edit files, and they forget everything when they exit. Mooshik stays active across your entire workspace. It maintains a persistent memory of your architecture, decisions, and constraints.

## The Triad Architecture

Mooshik divides responsibilities across three distinct systems.

```
+------------------------------------+------------------------------------+
|  MOOSHIK (AGPLv3)                  |  LAMBO (Apache 2.0)                |
|  Cowork Partner & Orchestrator     |  Living Graph Memory Substrate     |
|  * Ambient workspace awareness     |  * In-memory concept graph         |
|  * Fast local and hosted chat      |  * Write-behind persistence        |
|  * Web research and MCP tool hub   |  * Vector and keyword recall       |
|  * Terminal user interface         |  * SQLite and Postgres backends    |
+------------------------------------+------------------------------------+
|  CODING CONTRACTOR AGENT                                                |
|  * Performs surgical file edits in individual repositories              |
|  * Reads canonical constraints and architecture rules from Lambo       |
+-------------------------------------------------------------------------+
```

### 1. Mooshik (The Partner)
Mooshik manages the interactive session. It coordinates research, tool calls, and user interactions. It passes context to coding agents rather than pretending to write all code itself.

### 2. Lambo (The Memory Core)
Lambo provides a durable knowledge graph. It extracts concepts, tracks relations, and preserves facts across restarts.

### 3. The Coding Contractor
When you need code refactoring, Mooshik summons specialized coding agents. These agents execute edits under the architectural constraints that Lambo provides.
