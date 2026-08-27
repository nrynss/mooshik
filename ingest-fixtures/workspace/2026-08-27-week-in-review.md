# Week in review — Aug 21 to Aug 27

Thursday, sitting down to actually look at the week as a shape instead of
living inside it day by day.

## The arc

- **Friday (21st)** — Closed the Windpipe overflow decision:
  block-the-writer over drop-oldest. Felt good to finally land something
  that had been sitting on the backlog since July's retry-storm
  postmortem.
- **Saturday (22nd)** — Ran the backpressure experiment on my own time,
  mostly out of curiosity, confirmed the ring behavior empirically instead
  of just trusting the spec I'd written the day before. Good instinct, in
  hindsight — meant I actually understood the failure mode before I had to
  watch it happen for real.
- **Sunday (23rd)** — Deliberate rest day. Long ride, called home, read,
  tried to stay off the laptop and mostly succeeded.
- **Monday–Tuesday (24th–25th)** — Staged rollout, flag work, staging
  bake. Quiet on my end; the boring middle of a change is supposed to be
  boring. Also the week drinks with Iris and Marcus that I ended up
  bailing on Wednesday were originally supposed to happen Tuesday and got
  pushed once already before the incident pushed them again.
- **Wednesday (26th)** — The incident. Prod flip at 09:14, ring saturation
  at 09:24 during an unrelated audit sink reconnect issue, roughly 27
  minutes of degraded task dispatch before we killed and restarted the
  wedged reader. No data loss — the design held exactly as intended. Root
  cause turned out to be a stale Quillstone-cached build, not the Windpipe
  change itself. Also missed a call from Mom right in the middle of it and
  didn't call back properly until yesterday evening.
- **Thursday (27th, today)** — Recovery and follow-through. Closed out the
  postmortem doc, a cache-key decision record, and a small Mooshik
  hardening fix from a near-miss during the incident. Rode again in the
  afternoon. Finished the novel I'd been slow-reading all month. Iris and
  Marcus came over for the rescheduled dinner.

## What actually mattered

The headline for the week isn't the incident, even though it's the thing
that'll get remembered. It's that a design decision made under no time
pressure on a quiet Friday behaved exactly as specified under real
pressure five days later. That's the actual win. The incident was a
build-hygiene failure wearing a Windpipe costume — the bus did its job.

Second thing that mattered: the near-miss with the credential in the
incident-channel log paste. Nothing happened, and specifically nothing
happened because the boring infrastructure — the redaction scanner in
Mooshik's ingest path — did exactly what it was supposed to do. Spent this
morning making sure I could actually prove that instead of just assuming
it. Worth writing down as a principle rather than trusting it stays true
by accident:

> Secrets never enter the graph: the vault is the only place a credential value lives.

## What I'd do differently

- Should have asked, back on the 21st, whether anything outside the
  source tree could silently invalidate a cached build. Didn't occur to me
  until it caused an incident. Adding "does this dependency live outside
  the repo" to my mental checklist for future design docs.
- Let Priya's on-call message sit for over a week. Not a technical
  failure, just a me failure. Replying today, for real this time.
- Ran on six-ish hours of sleep for most of the week leading into
  Wednesday, which almost certainly made the incident response slower and
  grumpier than it needed to be. Correlation I can't prove, but I believe
  it.
- Let the review board's relitigating get under my skin more than it
  should have today. It's a Monday-meeting problem, not a today problem,
  and I spent more energy being annoyed about it than the thing deserves.
- The Mom call this morning helped but the actual visit-length question is
  still sitting there unresolved, and I know I'm just going to keep
  half-avoiding it until someone forces the actual conversation. Probably
  me who has to force it. Not this week.

## Numbers, for the record

- 1 design decision, 1 empirical validation, 1 prod incident, 3 follow-up
  documents, 2 bike rides, 1 novel finished, 1 rescheduled dinner that
  actually happened.
- ~27 minutes of degraded Zephyr task dispatch, 0 messages lost — the ring
  did exactly what the 08-21 doc said it would.
  The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping.
  Wednesday proved it under conditions nobody chose on
  purpose.

## Going into next week

- Quillstone cache-key fix ships tomorrow.
- Audit whether other services have config living outside their source
  tree (flagged in the cache decision doc, not doing it this week).
- Reply to Priya. Today. Actually today.
- Figure out the actual visit-length conversation with Mom before it turns
  into a bigger thing than it needs to be.
- Otherwise: quiet week, please.
