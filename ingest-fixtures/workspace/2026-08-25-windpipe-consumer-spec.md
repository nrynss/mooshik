# Spec fragment: read-only debug consumer for Windpipe

Draft, not final. Sketching this out after this morning's overflow debugging session made it obvious we need a way to inspect ring contents without adding another real reader that counts against consumer lag.

## Problem

Currently the only way to see what's actually sitting in the Windpipe ring during an incident is to read the Cloud SQL persistence table, which lags the live ring by up to 250ms and doesn't show messages that haven't been flushed yet. During this morning's stall, Priya and I both wanted to just *look* at the ring directly and couldn't.

## Invariant to preserve

Any new consumer type must not change ring semantics. Restating the core rule so it's in this doc too, not just in the readers' heads:

The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.

A debug consumer must not become a fourth "real" reader that this ceiling has to account for — if it does, we've just made the 512 limit effectively tighter for the readers that matter (retry-dispatch, audit-log, metrics-sampler).

## Proposed shape

A **shadow reader** mode:

- Attaches to the ring at the current write cursor, never behind it — no backlog, no lag metric.
- If it falls behind (can't keep up with write rate), it silently drops its own view rather than blocking the writer or counting toward the 512 ceiling. This is the opposite of normal reader behavior on purpose — a debug tool that could itself cause backpressure is worse than useless.
- Read-only at the API level; no ack, no offset tracking, no persistence.
- Exposed via a local socket on the `zephyrd` process, not over the network — this is a `kubectl exec`-and-attach tool, not a service.

## Open questions

- Does a shadow reader need its own slot in the writer's fan-out list at all, or can it tap the same memory the flush-to-Cloud-SQL path already reads? Leaning toward the latter — reuse the flush path's read, avoid a second read path entirely.
- What happens if two people attach shadow readers at once during an incident? Probably fine, both are just reading the same memory, but want to confirm there's no lock contention with the real writer path before shipping this.
- Naming — "shadow reader" vs "tap" vs "ring viewer." No strong opinion yet, will bikeshed with Priya since her team is the one who'll actually reach for this at 3am.

## Rough sketch of the protocol

```
attach  -> zephyrd opens local socket, one connection per shadow reader
stream  -> newline-delimited JSON, one message per line:
           {"seq": 88213, "ts": "...", "topic": "...", "payload_bytes": 412}
detach  -> either side closes the connection, no cleanup needed server-side
```

No auth beyond "you can exec into the pod," which matches how the team already treats other local debug sockets on zephyrd. Not exposing this over the network, ever — if someone needs remote access to ring contents that's a different, much more carefully scoped feature.

## Not in scope for this fragment

Filtering, formatting, or any kind of query language for the ring contents. First version should just dump raw messages to stdout with sequence numbers. Anything fancier is a v2 problem.

## Next step

Rough out the local-socket protocol (probably just newline-delimited JSON, one message per line) and get a prototype in front of Priya before writing the real spec doc. This fragment is just to get the invariant and the shape down while it's fresh from this morning.
