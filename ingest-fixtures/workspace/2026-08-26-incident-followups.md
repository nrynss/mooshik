# Follow-ups — Windpipe canary shard incident, 2026-08-26

Pulled together this evening while it's fresh, ahead of tomorrow's
postmortem review with Priya and Abhi. Owners assigned where obvious,
otherwise flagged for tomorrow's discussion.

## P0 — before canary re-expands past 4%

- [ ] Separate Windpipe write lane for backfill-class jobs vs. interactive
      task writers, so a bulk job filling the ring can't stall unrelated
      task completions. (Neom, design sketch by Friday, this is the real
      fix, everything else is a stopgap.)
- [ ] Task-completion latency alert on Zephyr, not just Windpipe reader-lag
      alerts. We found the 4-second p99 by looking, we should have been
      paged on it directly. (Priya, ticket by tomorrow)
- [ ] Reconcile the two drop-count numbers from the 10:29–10:41 window
      before either of them goes in the final postmortem doc. (Neom +
      Priya, tomorrow morning)

## P1 — this week

- [ ] Written rollback plan for any canary flag expansion, before the next
      one ships, not after. Should have existed this morning and didn't —
      that's the one process gap I'll actually own out loud tomorrow.
      (Neom, by Friday)
- [ ] Decide whether Wen's backfill job stays throttled at a quarter rate
      permanently or gets a real target number instead of "whatever
      stopped the bleeding this morning." (Wen + Neom, this week)
- [ ] Re-run the Saturday fairness-quantum measurement, this time with an
      artificial Windpipe stall injected, to get real numbers on how much
      of today's latency was the block itself versus queueing compounding
      on top of it. Suspect the queueing effect is the bigger number, want
      to actually know instead of suspecting.
- [ ] Staging backfill test needs to run at something closer to prod task
      volume — the whole reason this didn't surface Tuesday is staging
      load was too light for the queueing compounding to show up as
      anything but noise.

## P2 — lower priority, don't let it die but not urgent

- [ ] Write the ring-capacity-not-configurable decision somewhere more
      permanent and more visible than a design doc from five days ago,
      because apparently a written decision with reasoning attached
      doesn't stop it from getting relitigated mid-incident for the third
      time this quarter. Not sure a doc fixes a culture problem but it's
      the lever I have. (Neom, no real deadline, just tired of explaining
      it live)
- [ ] Audit whether any other Windpipe consumer besides the audit sink has
      the same "bulk job can starve everyone else" exposure. Cobalt
      Lantern's ingest path is the obvious next place to check, given
      Monday's incident already touched it once this week.
- [ ] Loop Tomas in on whether the frontend team wants a task-latency
      indicator on the live view even though it reads a snapshot — probably
      not needed, confirm and close either way so it stops being an open
      question.

## Not doing

- Not making ring capacity configurable per deployment. Still no. Writing
  this here too, for the record, in the file most likely to actually get
  read next time someone asks.
- Not rolling the canary back further than 0% on the affected shard —
  0% is fully off, there's nowhere further to roll back to.

## Tomorrow

Postmortem review with Priya and Abhi, 10am. Bringing this list and the
open design question about whether the canary re-expands with the lane
split in place or waits for a full redesign. I don't have a strong opinion
yet and that's fine, haven't earned one after a day like today.
