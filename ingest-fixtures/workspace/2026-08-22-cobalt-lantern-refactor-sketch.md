# Refactor sketch: Cobalt Lantern ingest layer

Not writing code today, just sketching. Cobalt Lantern's ingest module has
been bugging me for weeks — the station-parsing code and the normalization
code are tangled together in one 900-line file and every time I touch one I
have to re-verify I didn't break the other.

## Current shape (as I understand it)

```
ingest/
  station_feed.go     -- fetches raw station readings, ~40 station sources
  normalize.go         -- unit conversion, dedup, gap-filling
  writer.go             -- writes normalized readings to the timeseries store
```

The problem isn't the file split, it's that `station_feed.go` does
normalization inline while it parses (there's a `toCelsius()` call buried
three levels deep in the XML-parsing loop for the NOAA-format feeds), so
`normalize.go` only handles about half the actual normalization work. The
other half is scattered and undiscoverable unless you already know it's
there.

## What I want instead

Three clean stages, each independently testable:

1. **Fetch** — raw bytes in, raw bytes out (plus source metadata: station
   id, fetch timestamp, format hint). No interpretation of the payload at
   all. This stage should not know what a temperature is.
2. **Parse** — format-specific (NOAA XML, METAR text, the two vendor JSON
   APIs we pay for) → a single internal `Reading` struct. All unit
   conversion happens here and only here, so `toCelsius()` type calls live
   in exactly one place per format, not scattered through fetch code.
3. **Normalize** — dedup against what we already have for that station,
   fill small gaps (under 3 missing readings) by interpolation, flag larger
   gaps instead of guessing. Writes are a separate concern from this, stays
   in `writer.go` as-is, that part's actually fine.

## Why now

Genuinely just annoyance-driven — I went to add a fifth vendor feed on
Thursday and gave up after twenty minutes of trying to figure out where a
new format's parsing should live. That's a signal the current structure is
actively costing time, not just aesthetically displeasing.

## Risk / scope check

- Station feed configs (which stations map to which format) shouldn't need
  to change at all — this is purely internal restructuring, not a behavior
  change, so it should be safe to do incrementally.
- The one thing I'm nervous about: there's a special case in the current
  `station_feed.go` for stations that report in Fahrenheit but with a
  vendor-specific rounding quirk (rounds to nearest 0.5 instead of nearest
  0.1). If I move conversion into the Parse stage I need to carry that quirk
  with it, not lose it. Grep for `roundQuirk` before I start, make sure I
  find every instance.
- Tests: `normalize.go` currently has decent coverage (Wen wrote most of
  it), `station_feed.go` has almost none because it's mostly untestable in
  its current shape — mocking the inline conversion logic is awkward. That
  alone might be argument enough for the split.

## Rough plan, not scheduled

1. Extract `Reading` struct + Parse stage first, behind the existing Fetch
   code, no behavior change.
2. Move the scattered conversion calls into per-format parse functions one
   format at a time, starting with NOAA since it's the biggest and the one
   I understand best.
3. Backfill tests for Parse stage as I go — this is the actual payoff, not
   the restructuring itself.
4. Leave Normalize and Writer alone until Parse is solid — resist the urge
   to touch everything at once.

Estimate if I actually did this: maybe two focused days, not counting review
time. Not doing it this weekend. Just wanted the shape written down before
Monday brings something else that pushes it out another two weeks.
