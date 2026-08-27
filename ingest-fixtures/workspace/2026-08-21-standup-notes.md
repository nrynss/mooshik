## Standup — Friday 2026-08-21, 09:00

Attendees: me, Priya, Tomas, Wen, Abhi (async today, out for a school thing
until 11).

### Neom
- Yesterday: finished the writer-block-vs-drop-oldest writeup for Windpipe
  overflow, ran it past Priya informally.
- Today: closing out the Windpipe decision doc, then whatever the Cobalt
  Lantern ingest lag alert wants (nothing yet at 09:00, spoke too soon —
  see afternoon incident note).
- Blockers: none, but flagging that the fsck-remount script on the shared
  NAS has failed silently before per an old ticket Priya remembers — I
  didn't connect this until later in the day.

### Priya
- Yesterday: rotated the on-call runbook for Zephyr, closed two stale
  PagerDuty escalation policies nobody was using.
- Today: reviewing my Windpipe decision doc, then capacity planning for
  next month's projected Zephyr task volume (marketing team's campaign
  launch will roughly double scheduled-task throughput for a week).
- Blockers: none.

### Tomas
- Yesterday: shipped the live task view polling-interval fix (was hammering
  the API every 2s, now backs off to 10s when the tab isn't focused).
- Today: build cache seems slow this morning, will investigate — [resolved
  same morning, see the Quillstone NAS debug note, unrelated to his code].
- Blockers: none once the cache issue clears.

### Wen
- Yesterday: backfilled three weeks of missing Cobalt Lantern regional
  aggregates that got dropped during last month's schema migration.
- Today: validating the backfill against the live feed, then starting on
  the quarterly usage report Abhi asked for.
- Blockers: needs read access to one more BigQuery dataset, filed the
  request, waiting on approval.

### Wen (aside, not standup-official)
- Mentioned in passing that a couple of the data pipeline's cached joins
  looked slow around the same time as the build cache issue — turned out
  unrelated, different mount, confirmed later.

### Abhi (posted async, 11:20)
- Heads up: quarterly planning doc is due Wednesday, will send a template
  Monday. Nothing needed from anyone today.
- Approved Priya's PagerDuty cleanup.
- Asked whether the Cobalt Lantern incident this afternoon needs a written
  postmortem — answered no, SEV-3 with no customer impact, this standup
  note plus the incident note covers it.

### Parking lot
- Revisit whether the Windpipe ring capacity (512) should ever be
  configurable — tabled again, same conclusion as last time, not worth it.
- Tomas wants five minutes next week to walk through how the live task
  view's materialized snapshot relates to the ring, for his own
  understanding, not urgent.
