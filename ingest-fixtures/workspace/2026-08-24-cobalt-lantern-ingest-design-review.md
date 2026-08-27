# Design review — Cobalt Lantern ingest latency

Monday 11:00–11:40 (ran nine minutes over). Attendees: me, Wen. Priya was
supposed to join but was still buried in the Quillstone cache mess, sent
notes after.

## Problem statement

Wen's been tracking p99 latency on the Cobalt Lantern forecast-provider
ingest for eight days. It's climbed from ~180ms to ~640ms over that window,
gradually, no single step change to point at. Not customer-visible yet
(downstream consumers have enough buffer that they haven't noticed) but it
will be within a week or two at this rate if nothing changes.

## What we looked at

- Provider-side latency: ruled out first. Wen pulled the raw response
  timing from the provider's own status page and it's flat. Whatever's
  slow, it's on our side.
- Ingest queue depth: this is where it gets interesting. The queue depth
  graph tracks almost exactly with the p99 climb. Something is filling up
  and staying fuller than it used to.
- Consumer side: the downstream normalizer that reads off the ingest queue
  hasn't changed in six weeks. Ruled out as the direct cause, though it may
  be a contributing factor if it's slower than it thinks it is under load —
  flagged for a separate look.

## The actual finding

Cobalt Lantern's ingest queue has the same shape as Windpipe — bounded ring,
block-on-full semantics, not drop-on-full. We made that same call for it
two years ago, before my time on the project, and it's held up fine until
now. The thing that's changed is upstream volume: a new region got added to
the forecast pull list five weeks back (I'd forgotten this, Wen reminded
me) and nobody resized the ring to match.

For comparison, this is the same principle we just wrote down for Windpipe
on Friday:

> The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.

Cobalt Lantern's ring is smaller — 128 slots — sized for the old region
count. It's not overflowing outright (we'd see hard blocking, not creeping
latency, if it were) but it's running close enough to full, often enough,
that producers are absorbing small stalls constantly. Death by a thousand
250-microsecond cuts, roughly.

## Decision

Bump the ring to 256 slots and re-measure. Not going all the way to 512 —
Wen's point, and a fair one, is that we don't actually know the new steady
state yet and doubling is a reasonable first move rather than guessing at
the final number. If 256 isn't enough we'll know within days, not weeks,
because the p99 trend has been consistent enough to extrapolate from.

Explicitly **not** doing right now: switching Cobalt Lantern to drop-oldest
semantics as a quick fix. It came up — briefly, I'll admit I was tempted
given how tired I am of thinking about ring sizing today — but it's the
same tradeoff we already rejected for Windpipe and rejecting it twice in one
week for the same reasons felt like a waste of a meeting. Wen agreed once
we said it out loud, but I did have to say it out loud, which is a little
frustrating given this exact argument got made in writing four days ago.

## Follow-ups

- Wen: resize the ring, ship behind a flag, watch p99 for 48h.
- Me: check whether the new region's pull frequency is even necessary at
  its current cadence, or if we're just fetching more often than the data
  actually changes. Separate question, lower priority.
- Priya: nothing needed from her here, looping her in for awareness only
  since it's adjacent to the reader-lag work.

Next design review only if 256 isn't enough. Otherwise this is done.
