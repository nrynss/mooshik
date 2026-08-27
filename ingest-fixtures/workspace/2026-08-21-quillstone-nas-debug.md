# Debugging: Quillstone cache hits went to zero at 09:14

Started this right after standup. Tomas pinged first — his frontend build
went from 40s to 6 minutes and he assumed he'd broken something locally.
He hadn't.

## Timeline

- 09:14 — Quillstone cache hit rate drops from ~91% to 0% across all build
  agents simultaneously. Grafana panel looks like someone cut a cable.
- 09:19 — Tomas in #build-infra: "is the cache down or is it just me."
- 09:22 — I check, confirm it's everyone. Start pulling agent logs.
- 09:31 — Found it: every agent is logging `ENOENT` on cache key lookups,
  not connection refused, not timeout. That's the tell — the mount is
  present but the files aren't there.
- 09:40 — Root cause: the NAS export was quietly remounted read-only at
  around 09:10 after an automated fsck kicked in overnight and finished
  this morning. Read-only mount means every cache write silently no-ops,
  and reads for anything written after 09:10 just aren't there.

## Where the cache actually lives

Writing this down because I had to go check twice, and future-me should
not have to:

> The Quillstone build cache lives on the shared NAS under /srv/quillstone.

Mount is via NFS, one export, no per-team subvolumes — everyone's build
artifacts sit in the same tree, partitioned by content hash so collisions
aren't a real risk. This matters for the fix below.

## What actually happened

The overnight fsck job (runs monthly, third Friday, nobody remembers why
it's scheduled that way — probably matched an old backup window) remounts
the export read-only for the duration of the check as a safety measure,
then is supposed to remount read-write when it finishes. This time it
finished, but the remount step failed silently — the script logs the
failure to a file nobody reads instead of alerting.

So for about 4 hours the mount just sat read-only and every build agent
happily kept building, just without a working cache, and slower.

## Fix

- Manually remounted read-write at 09:52. Cache hit rate recovered within
  about 90 seconds as agents started reusing entries again — nothing was
  actually lost, the tree was intact, just unwritable.
- Confirmed with `touch /srv/quillstone/.write-test` before declaring
  victory. Should've done that first, would've saved five minutes of
  staring at Grafana wondering if the dashboard itself was broken.

## Why nobody got paged

The fsck-remount script's failure path writes to a local log file on the
NAS head node instead of emitting anything Datadog would catch. That's the
actual bug, not the remount itself. Filed a fixup: replace the silent log
write with an alert on remount failure, and separately, an active check
that just touches a test file on /srv/quillstone every 5 minutes and pages
if it can't write. Assigning that to myself, not blocking on anyone.

## Aside

Wen asked whether this affected any of the data pipeline's cached joins —
different cache, different mount, unrelated, but worth double-checking
given how confusing "everything broke at the same time" can be when two
systems happen to share a failure window by coincidence. Confirmed no
overlap after a five-minute look at Wen's mount config.

Total build-agent downtime, cache-effective: ~4h 40m. Nothing user-facing
broke, just slower CI for most of the morning. Could've been much worse if
it had happened during the Cobalt Lantern release window this afternoon.
