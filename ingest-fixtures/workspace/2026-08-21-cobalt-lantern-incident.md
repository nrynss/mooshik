# Incident: Cobalt Lantern regional gap, Pacific Northwest tiles

**Severity:** SEV-3 (degraded, not down)
**Detected:** 13:52
**Resolved:** 14:47
**Duration:** 55 minutes

## Summary

Cobalt Lantern served stale weather tiles for the Pacific Northwest region
(roughly Seattle to Portland coverage area) for about 55 minutes this
afternoon. No other regions affected. No outage, just staleness — tiles
kept serving last-known-good data instead of erroring, which is by design,
but 55 minutes of staleness during an active weather event in that region
is worse than it sounds.

## What happened

- 13:52 — Alert fires: ingest lag for the PNW upstream feed exceeds 10
  minutes (threshold is normally tripped by brief blips and self-resolves,
  so first response is "wait and see," which in retrospect cost 6 minutes).
- 13:58 — Lag still climbing. I start looking. Ingest worker for that feed
  is alive, not crash-looping, just not making progress.
- 14:05 — Found it: the upstream NOAA feed for that region started
  returning 200s with truncated payloads — valid HTTP, invalid content.
  Our parser was choking partway through, discarding the batch, and
  retrying the same truncated response every cycle. Classic "looks healthy,
  isn't" failure.
- 14:12 — Confirmed with Priya this wasn't on our side — checked our
  egress logs, nothing unusual about our request pattern that would
  explain upstream truncating responses to us specifically.
- 14:20 — Mitigation: forced ingest worker to fail loudly on truncated
  payloads instead of silently discarding, and fall back to the
  secondary NOAA mirror for that region only.
- 14:47 — Secondary mirror ingest catches up, lag back under a minute,
  tiles refreshing normally. Closed.

## Root cause

Upstream (NOAA primary feed) intermittently truncates large payloads under
load on their end — not something we control or can fix. Our parser's
failure mode on a truncated payload was to drop the batch and retry
identically, which meant we'd loop forever on the same bad response instead
of either erroring visibly or trying the fallback source.

## Why it took 6 minutes to start looking

The 10-minute lag threshold is right at the edge of "normal blip" for that
particular feed — it self-resolves maybe 1 in 3 times without anyone doing
anything. Not going to lower the threshold (would just add pager noise),
but I am adding a second condition: alert immediately, no wait-and-see, if
lag is climbing linearly rather than plateauing. That distinguishes a
transient hiccup from something actually stuck.

## Follow-ups

- [ ] Parser: treat truncated payload as a hard error, not a silent drop
      and retry. (me, today, small fix)
- [ ] Ingest worker: auto-failover to secondary mirror after N consecutive
      identical-error retries, not just on hard connection failure. (me,
      next week)
- [ ] Alerting: add the lag-slope condition described above. (me, next
      week, low priority relative to the two above)
- [ ] No customer comms needed — nobody noticed externally, tile staleness
      degrades gracefully and nobody filed a ticket. Confirmed with Abhi,
      no postmortem doc required for a SEV-3 with no customer impact, this
      note is sufficient.

## Note to self

This is the second time this quarter a "healthy-looking" truncated
response has caused silent staleness instead of a loud failure. Worth a
half-day sometime soon to audit every ingest parser in Cobalt Lantern for
the same failure shape rather than fixing them one incident at a time.
