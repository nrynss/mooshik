---
title: Memory & Lambo Graph
description: Understand the graph memory substrate and concept extraction model.
---

Mooshik relies on Lambo as its memory substrate. Lambo manages structured knowledge through an in-memory graph with write-behind persistence.

## Concept Types

Lambo stores knowledge as typed nodes rather than raw text dumps.

The system enforces five distinct concept types:

1. **`entity`**: Named components, services, tools, databases, and workspace resources.
2. **`logic`**: Invariants, algorithms, transformation steps, and business rules.
3. **`constraint`**: Explicit technical boundaries, port allocations, and version caps.
4. **`resource`**: File paths, documents, URLs, and artifact endpoints.
5. **`observation`**: Historical outcomes, test results, benchmarks, and performance metrics.

## Graph Relations

Lambo connects concepts using directed relations:

- **`parent_of`**: Represents hierarchical ownership and component containment.
- **`derives`**: Connects source observations or documents to the extracted concepts.

## Memory Lifecycle

### 1. Extraction
Extractors parse text, code commits, screenshots, or voice notes into typed concept nodes.

### 2. Derivation
Mooshik commits concepts into the graph using `lambo_derive`. The system computes vector embeddings and checks for existing nodes.

### 3. Recall
When you ask a question, Mooshik performs hybrid search. It combines semantic vector distance with keyword BM25 scoring and graph traversal.

### 4. Consolidation
The reflection engine merges near-duplicate concepts periodically. It redirects edges to the primary node and writes clean summary prose.
