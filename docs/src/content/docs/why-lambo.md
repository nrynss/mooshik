---
title: Why Lambo
description: Understand why Mooshik uses Lambo for structural graph memory and earned canonization.
---

Mooshik embeds [Lambo](https://nrynss.github.io/lambo) as its memory engine.

Lambo is an agentic graph memory substrate designed for multi-session AI operations. Mooshik leverages Lambo to give an ambient assistant long-term memory that lasts across months of work.

## The Limits of Flat Memory

Most AI memory implementations rely on simple vector databases. They embed every user sentence and retrieve similar paragraphs on demand.

This approach fails for long-term coworking:
1. **Extraction noise accumulates:** Every passing observation enters the database. Over time, outdated ideas clutter retrieval results.
2. **Structural dependencies are invisible:** A flat vector search finds textually similar sentences. It cannot identify whether an authentication schema is load-bearing infrastructure.
3. **No resistance to eviction:** Critical architectural constraints decay at the same rate as casual conversation notes.

## How Lambo Solves It

Lambo structures knowledge into a living property graph and validates concepts from structural evidence.

### 1. Earned Canonization

A concept is not important because an agent asserted it was. Concepts start in the candidate pool and earn promotion through survival across time and sessions.

Promoted concepts become canonical facts. Read the full promotion arithmetic in [Earned Canonization](/mooshik/canonization/) and on the [Lambo Canonization documentation](https://nrynss.github.io/lambo/canonization/).

### 2. Structural Blast-Radius Tracking

Lambo tracks incoming and outgoing dependency edges across entities, constraints, and resources.

When you prepare to modify a component, Lambo calculates its blast radius. If a concept supports ten other systems, Mooshik warns you before you break dependent systems.

### 3. Meaning-Based Hybrid Recall

Lambo combines vector similarity, structural graph traversal, and daemon importance scoring:

$$
\text{Recall Score} = 0.5 \times \text{Daemon Score} + 0.5 \times \text{Query Relevance}
$$

Concepts reached through graph traversal surface on structure alone. This enables recall to answer what rests on a component, rather than only what looks similar.

### 4. Pluggable Storage Backends

Lambo abstracts storage below a strict `GraphStore` seam:
- **SQLite:** Fast local storage for private offline operation.
- **PostgreSQL with pgvector:** Shared durable storage for multi-machine synchronization.

Read the official [Lambo documentation](https://nrynss.github.io/lambo) to explore the memory engine in depth.
