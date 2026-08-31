---
title: Reflection and Synthesis
description: Consolidate memory, merge duplicate concepts, and synthesize timeline prose.
---

`mooshik reflect` is a one-shot consolidation command. It synthesizes summary prose for the terminal pane and merges duplicate concepts in the memory graph.

Run reflection on your current session:

```bash
mooshik reflect
```

## Synthesizing Timeline Prose

While you work, the workspace watcher and companion record discrete events: file modifications, commits, and short notes. The live pane displays these raw events in real time.

`mooshik reflect` reviews the day's events and writes descriptive prose concepts:
- **Daily mood:** Captures the tone and pace of the day's work.
- **Gutter summaries:** Four-word concise summaries displayed along timeline margins.
- **Trailing notes:** Synthesizes context from completed tasks.
- **Thread rationales:** Explains why a group of related changes occurred together.

These are written to the graph with a `mooshik-prose:` prefix. On the next 250 millisecond tick, the open terminal pane renders the new prose automatically.

## Merging Paraphrase Twins

When multiple tools or sessions record the same observation using slightly different wording, `mooshik reflect` consolidates them:

1. **Semantic clustering:** Identifies concepts with near-identical vector representations.
2. **Leader selection:** Chooses the most descriptive concept as the canonical representative.
3. **Edge rerouting:** Reroutes all inbound and outbound edges from duplicate concepts to the leader.
4. **Content preservation:** Preserves supporting notes and evidence from merged concepts.
5. **Audit logging:** Records an immutable audit row for every merged cluster.

## Safety and Idempotence

- **First-write-only design:** Reflection processes unanalyzed events and skips already consolidated time periods. Rerunning `mooshik reflect` on unchanged data is a safe no-op.
- **Dry-run simulation:** Inspect planned merges and prose generation without modifying the database:

```bash
mooshik reflect --dry-run
```
