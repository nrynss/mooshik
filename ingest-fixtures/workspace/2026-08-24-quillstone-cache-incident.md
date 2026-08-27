# Incident note — Quillstone cache corruption, 2026-08-24

Sev: minor (team-scoped, no customer-facing impact). Owner: me, paged myself
basically — Priya was heads-down on the reader-lag PR and I was the one who
noticed first.

## Timeline

- **09:41** — Clean checkout on a fresh branch takes 11 minutes instead of
  the usual ~90 seconds. Noticed because I was trying to rebase the Windpipe
  flag branch before the design review and got impatient.
- **09:47** — Checked #eng-build, Tomas already posted "is quillstone dead
  for anyone else" four minutes before I got there. So it wasn't just me.
- **09:52** — Confirmed: CI runners across at least three repos (zephyr,
  cobalt-lantern-ingest, and the frontend monorepo) are all missing cache
  hits and rebuilding from scratch. Every job queued since ~09:15 is running
  long.
- **10:03** — Found it. A subset of cache entries under the windpipe build
  target have non-matching content hashes — the manifest says one thing, the
  blob on disk is something else. Looks like a partial write that didn't get
  cleaned up, possibly from Friday's flag-branch build racing with the
  weekend's scheduled cache GC.
- **10:15** — Abhi pinged asking if this blocks the 11:00 design review.
  Told him no, moving forward with just Wen and me, Priya can read notes
  after.
- **10:40** — Manually evicted the corrupted entries. No checksum validation
  on read, which is the actual root cause here — a bad blob gets served
  as if it were good, and the build that consumes it fails somewhere
  downstream in a way that doesn't obviously point back to the cache.
- **11:05** — CI hit rates back to normal (~94% across the three repos).
  Closing this as resolved, opening a follow-up for the real fix.

## Where the cache actually lives

For anyone who forgets this like I do every few months:

> The Quillstone build cache lives on the shared NAS under /srv/quillstone.

Entries are content-addressed, keyed by a hash of the build target's inputs.
The manifest and the blob are separate files on purpose (so partial writes
to one don't necessarily corrupt the other) but that separation only helps
if something actually checks they agree, which as of this morning, nothing
does.

## Root cause (working theory, not confirmed)

Most likely a write from the flag-branch CI run Friday afternoon got
interrupted — runner got reclaimed mid-write during the weekend GC sweep —
and left a manifest pointing at a blob that never finished landing. GC
doesn't currently guard against evicting something mid-write because nobody
expected a write to still be in flight during a scheduled GC window. Small
window, unlucky timing, but not actually rare enough to call a fluke —
this is at least the third time in two months someone's mentioned "cache
felt slow" right after a GC run.

## Follow-ups

- Add a checksum check on cache read, not just write. This is the actual
  fix. Filed, no owner yet — probably me, next week, after the Windpipe
  rollout is done.
- GC should not evict an entry whose write hasn't been confirmed complete.
  Feels like an obvious miss in retrospect.
- Quick manual "did we lose time from this" estimate: roughly 25 build-minutes
  wasted per repo across the affected window, so under two hours total
  team-wide. Not worth a full postmortem, but worth writing down since I
  keep forgetting this happened last time too.

## Random annoyance

This is the second time this quarter I've had to explain to someone what
"content-addressed" means for the cache before they'll believe the corruption
theory over "the build server is just having a bad day." Not naming names.
It's fine. It's Monday.
