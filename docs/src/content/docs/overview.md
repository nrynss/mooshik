---
title: What is Mooshik
description: Understand Mooshik's design philosophy, origins, and ambient coworking workflow.
---

Mooshik is an ambient, local-first cowork partner and workspace orchestrator. It sits with you while you work, remembers everything across projects, and acts on your behalf.

Traditional coding agents act like temporary contractors. You summon them for a specific prompt, they edit files in a single repository, and they exit. When they finish, they forget what happened. Cloud chatbots keep conversation threads but remain disconnected from your daily filesystem changes.

Mooshik takes a different approach. It stays open in a terminal pane beside your editor. It monitors your workspace activity, tracks decisions, and maintains continuity across weeks of work.

## The Myth and the Pitch

The name reflects the system architecture.

In ancient myth, **Lambodaran** (Ganesha) is the deity of intellect, memory, and the removal of obstacles. **Mooshik** is his companion and vahana, the mount that carries him into the world.

Lambo represents the vast, durable memory substrate. Mooshik is the companion that carries memory into action. The Sanskrit root *mus* means to extract and gather. Mooshik scurries through your workspace and tools, gathers context, and stores it in Lambo's graph.

## Coworking vs Command Execution

| Dimension | Command Agents | Cloud Chatbots | Mooshik |
| :--- | :--- | :--- | :--- |
| **Relationship** | Master to contractor | Question to answer | Cowork partner |
| **Lifecycle** | Ephemeral process | Ephemeral session | Continuous pane |
| **Scope** | Single repository | Single conversation | Whole workspace |
| **Memory** | None across sessions | Flat prompt context | Living graph memory |
| **Awareness** | Explicit invocation | None | Ambient file and git tracking |

## Three Layers of Collaboration

1. **The Ambient Companion (Mooshik):** Manages the terminal pane, routes user requests, scans for security boundaries, and tracks workspace events.
2. **The Memory Substrate (Lambo):** Maintains an in-memory knowledge graph, computes recurrence across time, and persists facts to SQLite or PostgreSQL.
3. **External Tools and Contractors:** MCP servers provide web research, multimodal artifact extraction, and delegated coding execution under standing memory constraints.

## Next Steps

- Read [Why Lambo](/mooshik/why-lambo/) to learn how structural memory works.
- Follow [Quickstart](/mooshik/quickstart/) to set up Mooshik and launch the pane.
