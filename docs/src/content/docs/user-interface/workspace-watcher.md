---
title: Workspace & Git Watcher
description: Automatic observation of workspace files and git commit history.
---

Mooshik includes an ambient workspace watcher that extracts knowledge quietly while the terminal UI remains open.

## File System Observation

The watcher monitors the current working directory for file updates.

### File Ingestion Rules
- Supports documentation formats: `.md`, `.markdown`, `.txt`, and `.rst`.
- Ignores build outputs like `target/` and `node_modules/`.
- Skips temporary files and symlinks.
- Scans file contents for secrets before saving any reference.

## Git Commit Tracking

The watcher treats git commits as high-value intent signals.

When you create a commit, Mooshik extracts:
- Commit subject and message body.
- Changed file names and resource references.
- Original author timestamp to maintain accurate timeline ordering.

Mooshik ignores internal `.git/` object churn while capturing real commit milestones.

## Debounce and Coalescing

Rapid file modifications from formatters or build tools fire many events quickly.

The watcher debounces incoming events and coalesces rapid bursts into a single derivation call. This prevents unnecessary vector embedding calls and reduces database lock contention.
