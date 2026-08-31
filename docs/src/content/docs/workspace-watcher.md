---
title: Ambient Workspace Awareness
description: Automatic observation of workspace files, git commits, and directory polling costs.
---

Mooshik includes an ambient workspace watcher that tracks files and git commits while the terminal pane remains open.

The watcher runs as a background task linked to the lifetime of `mooshik tui`. When you close the terminal interface, the watcher stops immediately.

## Metadata-Only Observation

The watcher records that files changed, never what they contain.

- **File change concepts:** When you modify a file, the watcher emits a concept formatted as `workspace file changed: <filename>` along with the file modification timestamp (`mtime`). Raw file text is never stored in the memory graph.
- **Secret scanning gate:** The watcher reads file contents solely as a security gate. If a credential pattern is detected anywhere in the file or path, the event is dropped silently.
- **Git repository boundary:** When the watcher encounters a directory containing `.git`, it captures commit metadata (commit SHA, author timestamp, and commit message). It does not descend into working tree files within that repository.

## Polling and Debounce Rules

- **File extension allowlist:** Tracks `.md`, `.markdown`, `.txt`, and `.rst` files only.
- **Excluded directories:** Ignores `.git`, `.ingest`, `.venv`, `venv`, `node_modules`, `target`, `__pycache__`, and `.pytest_cache`.
- **Symbolic links:** Ignored. The watcher never traverses symlinks.
- **Adaptive polling:** Checks the filesystem on an adaptive 100 to 250 millisecond interval.
- **Burst debounce:** A 250 millisecond debounce timer coalesces rapid edits into a single event.
- **Serialized writes:** All concept derivations route through the shared `WriteLane` actor.

## Positional Workspace Root

There is no configuration key for the workspace root. Mooshik watches the working directory from which you launch `mooshik tui`.

Choosing the right directory is important for polling efficiency.

### Directory Walk Performance

The following benchmarks show walk costs measured on a development machine on 2026-08-31:

| Launched From | Loose Files Seen | Nested Repos Skipped | Dirs Walked | Walk Duration |
| :--- | :--- | :--- | :--- | :--- |
| Single repo (`~/work/mooshik`) | 213 | 0 | 109 | 2 ms |
| Parent of repos (`~/work`) | 4,712 | 32 | 3,919 | 58 ms |
| Home directory (`~`) | 28,202 | 84 | 156,851 | 2,487 ms |

### Recommended Usage

- **Parent directory (`~/work`):** Recommended for daily work. Allows Mooshik to monitor multiple projects while staying well within the 250 millisecond polling budget.
- **Single repository (`~/work/mooshik`):** Suitable when focusing exclusively on one codebase.
- **Home directory (`~`):** Do not launch from home. Walking an entire home directory takes roughly 2.5 seconds per poll cycle, exceeding the polling budget tenfold.
