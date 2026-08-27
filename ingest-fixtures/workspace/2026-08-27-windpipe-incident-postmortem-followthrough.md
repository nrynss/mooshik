# Windpipe prod incident, Wed Aug 26 — follow-through

Thursday, catching my breath and closing loops from yesterday. Writing this
before the details go soft.

## Timeline (confirmed against the reader-lag dashboard and PagerDuty log)

- 09:14 — block-the-writer change flipped to prod for Windpipe (final step
  of the rollout from last Friday's decision record, after a clean 24h bake
  in staging).
- 09:16 — audit sink reader started throwing reconnect errors against Cloud
  SQL. Not the flag flip's fault directly — coincidence timing that made it
  look worse than it was for the first ten minutes.
- 09:19 — reader-lag alert fired (the one Priya asked for after the design
  review — good, it worked).
- 09:21 — I get paged. Priya already in the incident channel.
- 09:24 — ring occupancy crosses 512.
  The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.
  That's exactly the failure mode we designed for, not a surprise, but it's
  different watching it happen in prod with real task dispatch stalling
  behind it.
- 09:24–09:41 — Zephyr task dispatch effectively frozen. Nothing lost,
  nothing corrupted, just queued and late. ~1,100 tasks backed up by the
  time we cleared it.
- 09:31 — Priya and I agree the audit sink itself is wedged, not just slow
  — it's not draining even under zero contention. Decide to kill and
  restart it rather than wait.
- 09:38 — restarted audit sink resumes from its last acked offset, starts
  draining at full speed.
- 09:41 — ring occupancy back under 100, writers unblocked, dispatch
  catches up within another six minutes.
- 09:47 — all-clear posted. Total user-visible impact: ~27 minutes of
  elevated task latency, no data loss, no incorrect scheduler state.

## Root cause (short version — full writeup in the cache decision doc)

The restarted audit sink came back up on a stale build. Someone (not naming
names, could've been me) redeployed off a Quillstone-cached artifact that
predated a connection-pool fix from three weeks ago, and the cache key
didn't account for a config change that should have busted it. Old binary,
old reconnect bug, wedged reader. Full detail and the actual fix in today's
cache-key doc.

## Action items — status as of today

1. Reader-lag alert — already existed, worked exactly as designed. Closing
   this one, no action needed, just noting it paid for itself.
2. Cache key fix so a stale Quillstone artifact can't silently get
   redeployed again — in progress, see decision record, targeting EOD
   tomorrow.
3. Runbook entry for "audit sink wedged, ring filling" — writing this
   afternoon, folding into the Zephyr onboarding doc while I'm in there
   anyway.
4. Near-miss: a log snippet pasted into the incident channel almost carried
   a credential into Mooshik's memory. Separate hardening doc, already
   shipped a fix this morning.
5. Abhi wants a two-paragraph summary for the leadership sync tomorrow —
   drafted, sitting in a draft doc, sending after I reread it once more
   with fresh eyes.

## Friction, for the record

Not everything about today was clean closure, and I'd rather write this
down honestly than pretend the week wrapped up tidy.

- The platform review board wants a formal sign-off meeting for the
  block-the-writer change even though it already went through design
  review, staging bake, and now a live incident that proved it works as
  specified. That's the same decision getting relitigated a third time by
  a group that wasn't in the room for any of the first two. Mildly
  furious about it, mostly because it's a diary-entry kind of complaint —
  nothing to actually do except sit through the meeting Abhi is booking
  for Monday.
- Wen asked me this morning for the throughput numbers from Saturday's
  experiment, which are the numbers, verbatim, in the doc I linked in the
  incident channel twice yesterday. Not annoyed at Wen exactly, more at
  the general pattern of writing something down carefully and having it
  not actually reduce the number of times I have to say it out loud.

## Notes to self

- The design held. That's the actual headline here — the system did
  exactly what the Aug 21 decision said it would do, under real load,
  during a real failure. Nothing about the block-the-writer semantics
  needs to change.
- What needs to change is how we build and cache things, not how the bus
  behaves. Good problem to have, comparatively.
- Slept badly last night running the timeline in my head. Today is about
  closing these out cleanly so I can stop rehearsing it.
