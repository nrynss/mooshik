# Postmortem (DRAFT — not reviewed) — Windpipe canary shard task latency

**Status:** draft, mine only, sending to Priya and Abhi tomorrow before
calling it final. Blameless, but I'm allowed to be annoyed in my own draft
before I sand it down for the shared doc.

**Severity:** SEV-2
**Detected:** 09:42
**Resolved:** 11:12 (declared closed 11:40 after monitoring window)
**Duration:** 90 minutes degraded, canary shard only (~4% of prod task
volume for that window)

## Summary

The Windpipe block-writer behavior — canaried to one prod shard Tuesday
night after 24h clean in staging — worked exactly as designed when the
audit sink reader fell behind during a scheduled backfill burst. No
messages were lost. What I didn't anticipate: blocking writers also stalls
task-completion writes for *unrelated* tasks sharing that Windpipe
instance, which keeps them on the Zephyr runqueue instead of clearing, and
they then pay full fairness-quantum queueing delay on every extra
round-robin pass. p99 task latency on the canary shard hit just over 4
seconds before mitigation.

## Impact

- One prod canary shard, roughly 4% of total task volume in the affected
  window.
- No data loss, no dropped messages — the design goal from Friday's
  decision held.
- Task latency degraded for ~90 minutes; no full outage, nothing customer-
  facing broke, but any task depending on tight latency on that shard was
  affected. Tomas confirmed the frontend live task view wasn't touched,
  reads a snapshot not the ring.

## Timeline

Condensed here, full detail in the separate timeline note. Backfill batch
kicked off 09:40, alert at 09:42, root cause identified by 10:09, mitigated
10:20–11:05, recovered 11:12, closed 11:40.

## Root cause

Direct cause: a scheduled re-ingest backfill (Wen's job, catching up the
residual PNW gap from Monday's Cobalt Lantern incident) pushed a burst of
writes through Windpipe on the canary shard fast enough that the audit
sink reader couldn't keep pace. Per spec:

> The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.

That's correct behavior and it did its job — zero messages lost. The actual
failure is a gap in the design: when writers block, *every* writer on that
shard blocks, not just the backfill job's writer. Task-completion writes
for ordinary, unrelated tasks queued up behind the backfill burst, which
meant the scheduler kept treating those tasks as still-pending and
re-enqueued them into the next round-robin slice instead of retiring them.

## Contributing factor: the fairness quantum interaction

This is the part I actually didn't see coming, and it's the more
interesting failure mode. From Saturday's perf note: short tasks already
pay queueing delay under Zephyr's fixed-size scheduling, because the
scheduler round-robins in fixed slices regardless of actual work queued.
Today's incident is that same mechanism under adversarial load instead of
steady state. Every task stuck behind the Windpipe block wasn't just
waiting on the block — each additional round-robin pass it survived cost it
close to a full quantum, compounding the delay well past what the raw
Windpipe stall alone would explain. Staging never surfaced this because
staging's backfill test never ran at prod task volume, so the queueing
compounding effect stayed small enough to look like noise.

## Detection

Alert fired at 09:42 on reader lag, which is the right signal and fired
promptly. Task latency itself doesn't have its own alert yet — we found out
about the 4-second p99 by looking, not by paging. That's a gap.

## What went well

- Block-don't-drop worked. Genuinely zero data loss, which was the entire
  point of Friday's decision.
- Root cause found in under 30 minutes from page to identified mechanism.
- Rollback path (canary flag to 0%) existed and worked cleanly, even though
  the rollback *plan* wasn't written down yet when I needed it, which I'll
  come back to below.

## What went poorly

- No documented rollback plan for the canary expansion before it shipped
  this morning — I said at standup I'd write one and then didn't, because
  I got interrupted by the thing the rollback plan was for. That's on me.
- About 25 minutes into mitigation, the wider incident channel started
  relitigating whether Windpipe's ring capacity should be a configurable
  flag per deployment — the exact question Friday's design doc already
  closed out, with reasoning, in writing. I had to stop debugging to
  re-explain a decision that's sitting in a doc from five days ago instead
  of just linking it and moving on, because apparently nobody reads past
  the first paragraph. Not naming names in the real version of this doc.
  Naming names in my head.
- No latency-based alert on Zephyr task completion, only lag-based alerts
  on Windpipe readers. We got lucky that lag and latency correlated
  cleanly here; they won't always.

## Open question for tomorrow

Does the canary re-expand once we fix the completion-write coupling, or do
we redesign so backfill-class writers get a separate Windpipe lane from
interactive task writers? Leaning toward the second — feels like the kind
of thing that should have been obvious from the start, in hindsight, which
is exactly the kind of thing I always say after an incident and then
forget by the next one.
