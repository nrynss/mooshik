# Mitigation notes — live, Windpipe canary shard

Taken in real time during the incident, cleaning up typos only. Timestamps
are wall clock.

- **10:14** — Priya joins. We agree on the working theory fast: backfill
  burst filled the ring, writers blocking per Friday's design, blocked
  writers are also stalling unrelated task completions. Need to stop the
  bleeding first, understand the interaction second.
- **10:17** — Two options on the table: (1) throttle the backfill job's
  write rate so the ring never fills in the first place, or (2) pull the
  canary flag back to 0% and let block-writer behavior revert to the old
  drop-oldest path for that shard until we can fix the coupling properly.
  Priya's instinct is (2) first because it's the faster lever, (1) as a
  belt-and-suspenders follow-up.
- **10:20** — Throttled Wen's backfill job first — cut its write rate to
  roughly a quarter, via the existing per-job rate limit config, no code
  change, no redeploy. This alone should reduce ring pressure even before
  the flag change lands.
- **10:24** — Rate limit applied, confirmed via the job's own throughput
  metric. Ring occupancy on the canary shard still near the 512 ceiling —
  the backfill throttle helps future pressure, doesn't drain what's already
  queued.
- **10:27** — Rolled the canary flag back to 0% for the affected shard
  through the config service. This is a flip of an existing flag, not a
  deploy — took effect within about 15 seconds per the config service's
  own propagation metric.
- **10:29** — New writers on that shard now hit the old drop-oldest
  fallback instead of blocking. Confirmed via the reader-lag dashboard that
  message drops started happening at this point — expected, and
  acceptable for a few minutes given the alternative was compounding
  latency with no end in sight. Logged the drop count for the postmortem:
  written down separately, not repeating the number here since I want to
  double check it against Priya's copy before it goes in a doc anyone else
  reads.
- **10:33** — Aside, not really mitigation: the incident channel has started
  going in circles again on whether ring capacity should just be a
  configurable flag. It is not, we decided that Friday, it's in the design
  doc, I linked it twice. Muting notifications from that thread for the
  next twenty minutes, need to actually think.
- **10:41** — Ring occupancy on the canary shard starts dropping — backfill
  throttle plus flag rollback combined are draining the backlog faster than
  new writes are arriving.
- **10:53** — Ring occupancy back under 100 in-flight, well clear of the
  512 ceiling. Audit sink lag starting to recover, down from a peak I
  didn't get an exact number on to under 90 seconds.
- **11:00** — Task latency on the canary shard trending down hard, p99
  under 500ms and falling.
- **11:05** — Considered this the end of active mitigation. Nothing left to
  push on, now just watching.
- **11:12** — Audit sink lag under 30 seconds, task p99 under 100ms.
  Recovered.
- **11:20** — Checked the vault access log for the config service change at
  10:27, purely as incident hygiene — wanted to confirm the flag flip used
  the service credential from the vault and not anything pasted into the
  channel by hand during the scramble. It did. Nobody pasted anything they
  shouldn't have, but I wanted it checked, not assumed, given how fast
  10:20–10:29 moved.

  Secrets never enter the graph: the vault is the only place a credential value lives. That held today, same as any other day.
- **11:40** — Formally closed after 28 minutes of clean monitoring.

## Loose ends going into the postmortem

- Drop count during the 10:29–10:41 window needs reconciling between my
  number and Priya's before it's final — close but not identical, probably
  a counting-window difference, not a real discrepancy.
- Backfill job is still throttled at a quarter rate. Someone needs to
  decide whether that's the new permanent limit or a temporary one — not
  deciding that today, too tired to trust my own judgment on a number right
  now.
- Config service flag flip worked cleanly enough that I'm now slightly
  more relaxed about canary rollbacks in general than I was this morning,
  which is a dangerous thing to feel the same day it saved us. Writing it
  down so future-me remembers today wasn't actually easy, it just ended
  fast.
