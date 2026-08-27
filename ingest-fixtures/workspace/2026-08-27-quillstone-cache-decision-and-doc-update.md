# Decision: Quillstone cache keys must include config hash, not just source hash

Following up directly from yesterday's incident. This is the actual
root-cause fix, plus the doc updates that should have existed before any
of this happened.

## Background

The Quillstone build cache lives on the shared NAS under /srv/quillstone.
Cache keys today are derived from a hash of the source tree only —
commit-scoped, not deployment-scoped. That was a fine assumption back when
config lived in the same repo and changed in lockstep with source. It
hasn't been true for a while: the audit sink's connection-pool settings
moved into a separate config repo eight or nine months ago, and nobody
updated the cache key derivation to match.

## What went wrong

Three weeks ago someone landed a connection-pool fix as a config-only
change — no source diff in the audit sink's own repo, just a tuning value
in the config repo. Quillstone, hashing only source, saw an identical
cache key to the pre-fix build and happily served the old cached artifact
on every subsequent build. Yesterday's redeploy pulled that stale
artifact, which still had the old reconnect behavior that caused the
reader to wedge under a routine Cloud SQL blip. The block-the-writer
semantics did exactly what they were designed to do once the ring filled
— this was a build hygiene problem wearing a Windpipe costume.

## Decision

Cache key derivation changes from `hash(source tree)` to
`hash(source tree + resolved config)`. A config-only change now busts the
cache correctly. Considered and rejected:

- **Manual cache bust on config deploys** — relies on someone remembering,
  which is exactly the failure mode we just had. Rejected.
- **Separate cache namespace per config version** — technically cleaner
  but a bigger change to the Quillstone key schema than we want to take on
  this week under incident pressure. Revisit later if the combined-hash
  approach turns out to be too coarse (e.g. if it starts busting the
  cache on config changes that don't actually affect the build).

Combined hash it is. Straightforward, small diff, directly addresses the
actual gap.

## Rollout

- Land the key-derivation change behind nothing — this is a correctness
  fix, not a behavior change worth flagging.
- First build after landing will be a full cache miss for everything
  touched by a config repo change in the last nine months, which means a
  slow morning for CI. Warning Wen and Tomas ahead of time so nobody's
  confused when their branch takes 40 minutes instead of 6.
- Targeting EOD tomorrow, per the postmortem follow-through doc.

## Doc update — Zephyr onboarding page

Folding this into the onboarding doc while I'm already in the incident's
blast radius, since the "where does the build actually live" question
comes up from every new hire and the current doc doesn't answer it:

- Added a section explaining where the build cache lives, how keys are
  derived (updated for the fix above), and what "stale artifact" means in
  practice — pointing at yesterday's incident as the worked example,
  because a hypothetical never sticks the way a real story does.
- Added a runbook stub: "if a redeployed service is behaving like an old
  version of itself, check whether it actually rebuilt or served a cached
  artifact" — this should have been the first thing either of us checked
  yesterday and it wasn't, cost us maybe fifteen minutes of confusion
  before Priya thought to check the build hash against the commit.
- Left the reader-restart-from-offset semantics from the Aug 21 spec
  fragment alone — still accurate, no changes needed there.

## Loose end

Should probably audit whether any *other* services have config living
outside their source tree and are quietly relying on the same broken
assumption. Not doing that today. Adding to next week.
