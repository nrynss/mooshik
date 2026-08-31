---
title: WriteLane & Concurrency
description: How Mooshik manages single-writer leases and in-process concurrency.
---

Mooshik enforces clear concurrency boundaries to protect memory integrity and avoid optimistic write conflicts.

## The Single-Writer Lease

Only one process may hold write access to a given session at any time.

When `mooshik tui` or `mooshik chat` starts, it acquires an exclusive lease on the target database. If another process attempts to open the same session, Mooshik refuses with a clear conflict message and exits with status code 2.

The process releases the lease cleanly when the session shuts down. If the process crashes, the lease expires automatically after a short timeout.

## In-Process WriteLane

Within a single process, multiple tasks might attempt to derive concepts concurrently. For example, the workspace watcher and an active conversation turn may write simultaneously.

Lambo handles writes optimistically:
1. It plans under a read lock.
2. It generates vector embeddings across an asynchronous boundary without holding the lock.
3. It commits changes only if the graph epoch remains unchanged.

If the epoch moves during embedding generation, Lambo re-runs the plan. If contention exceeds eight attempts, the derive operation fails.

To prevent write failures under high activity, Mooshik wraps all in-process derives with a `WriteLane` mutex:

```rust
// Enter the serialized write lane before deriving concepts
let _lane = pane.writes().enter().await;
pane.memory().derive(&concepts, &parent_of).await?;
```

The `WriteLane` guarantees that internal tasks queue cleanly rather than competing for optimistic commits.
