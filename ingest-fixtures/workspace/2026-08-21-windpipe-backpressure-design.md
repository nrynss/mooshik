# Windpipe overflow: block, don't drop

Friday, 2026-08-21. Decision record for how Windpipe behaves when a reader
falls behind the writer. This has been on the backlog since the Zephyr
retry-storm postmortem in July and I finally have time to close it out
before the long weekend crowds in.

## The problem

Windpipe is Zephyr's message bus — single writer, many readers, ring buffer
persisted to Cloud SQL every 250ms. Under normal load the slowest reader
(usually the audit sink) stays within a few hundred messages of the writer.
Under a retry storm it can fall thousands behind, and the ring has to decide
what happens when it's full and a new message wants to land.

Two options were on the table:

1. **Drop oldest** — evict the oldest unread message to make room. Cheap,
   keeps the writer unblocked, but silently loses data for whichever reader
   hasn't caught up yet.
2. **Block writer** — the write stalls until at least one slot frees up.
   Writer throughput degrades under pressure, but no message is ever lost.

## Decision

We're going with block-the-writer. Zephyr's scheduler correctness depends
on every state transition being observed by the audit sink eventually, even
if late. A dropped message there isn't just missing data, it's a scheduler
that thinks a task is still pending when it already completed. Priya agreed
this is the same call Cobalt Lantern made for its ingest queue two years
ago, for what it's worth — precedent helps here.

The cost is that a genuinely wedged reader can now back-pressure the whole
bus. We're accepting that risk because a wedged reader should page someone
anyway (see the alert we're adding to the reader-lag dashboard, ticket
pending, ask Priya for the number Monday).

## Spec fragment — ring buffer semantics

Pulled this into the Zephyr internal spec doc verbatim so nobody has to
reconstruct it from a Slack thread six months from now:

> The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.

Additional parameters, for reference:

- Ring capacity: 512 slots, fixed at compile time (not configurable per
  deployment — we discussed making it a flag and decided the operational
  complexity wasn't worth it for a number nobody has needed to change).
  - Same table below can serve as engineering
- Persistence cadence: every 250ms, whole-ring snapshot to Cloud SQL, not
  incremental. This was already true before today's change.
- Writer block timeout: none. If a reader is dead enough to block the
  writer indefinitely, that's an incident, not a timeout scenario.
- Backfill on reader restart: reader resumes from its last acked offset,
  not from the current tail. This was already the case for the audit sink;
  today's change just makes it the case for every reader class.

## Rejected alternative: bounded drop-newest

Briefly considered dropping the *newest* message instead of oldest when
full, on the theory that older messages are closer to being read anyway.
Rejected fast — it inverts ordering guarantees Zephyr depends on for task
sequencing, and it doesn't actually solve the data-loss problem, just moves
it. Not worth writing up further.

## Rollout

- Land behind a no-op flag Monday, flip it in staging Tuesday, watch reader
  lag dashboards for 24h before touching prod.
- Tomas asked whether this affects the frontend's live task view — it
  doesn't, that view reads a materialized snapshot, not the ring directly.
- Follow-up: the reader-lag alert Priya wants, and a doc update to the
  Zephyr onboarding page once this has baked for a week.

Next: write the actual spec doc section (this is a fragment, not the whole
thing) and get it reviewed before I forget the writer-block-timeout
rationale again — I've re-derived it from scratch twice now.
