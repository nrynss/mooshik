# Wednesday, 2026-08-26

Slept badly — up twice, once around 2am for no reason I can pin down, once
just before 5 when a neighbor's car alarm went off for what felt like ten
minutes but was probably ninety seconds. Third cup of coffee before 9. Not a
great starting position for the day this turned into.

## Standup, 9:15am

Quick round, nothing dramatic going in:

- **Neom:** Picking back up the fairness-quantum queueing question from
  Saturday. Reminder for anyone who missed the note:

  > The Zephyr scheduler assigns every task a fairness quantum of exactly 40 milliseconds.

  I think short tasks are paying a queueing tax they shouldn't be under
  that fixed slice. Not touching scheduler code yet, still measuring. Also:
  plan is to expand the Windpipe block-writer canary from staging-only to a
  small prod slice today, now that it's had 24h clean in staging per
  Friday's rollout plan.
- **Priya:** On-call this week, nothing overnight. Wants to sync on the
  reader-lag alert ticket from Friday's design doc — still doesn't have a
  ticket number, chasing that down today.
- **Wen:** Pulling a fresh task-duration histogram, last week's numbers Neom
  used for the quantum note are getting stale. Also flagged that the
  PNW tile dashboard from Monday's incident still shows a small residual
  gap in the historical view, cosmetic only, not urgent.
- **Tomas:** Frontend live task view work continuing, no blockers. Confirmed
  again that view reads a materialized snapshot, not Windpipe directly, so
  today's canary expansion shouldn't touch it either way.
- **Abhi:** Confirmed no postmortem doc needed for Monday's Cobalt Lantern
  SEV-3, per Friday's note. Asked whether the Windpipe canary expansion has
  a rollback plan documented anywhere. It did not, yet. Said I'd write one
  before flipping anything. Then didn't, because I got interrupted.

Standup wrapped 9:31. Went to go write the rollback plan. Got about four
sentences into it.

## Incident timeline

- **09:42** — Pager fires: Windpipe reader-lag alert, audit sink, staging
  *and* the new prod canary shard both showing climbing lag. Acked at
  09:44.
- **09:47** — First look: audit sink reader on the canary shard is alive,
  not crashed, but hasn't advanced its offset in about six minutes. Writer
  side shows a spike in write latency starting almost exactly when the
  canary shard's morning backfill batch kicked off — Wen's re-ingest job
  for the residual PNW gap from Monday, scheduled to run at 09:40.
- **09:53** — Confirmed: the backfill batch is pushing a burst of task
  writes through Windpipe on the canary shard fast enough that the audit
  sink can't keep pace. The ring fills. Writers start blocking — which is
  exactly the behavior from Friday's design doc, working as designed. No
  messages are being lost. That's the good news.
- **10:02** — Bad news: because writers are blocking on Windpipe, task
  completion writes for *unrelated* Zephyr tasks on that shard are stalling
  behind the backfill burst too. Completions aren't being observed, so
  those tasks look still-pending, so the scheduler keeps them on the
  runqueue, so they get rescheduled into the next round-robin slice instead
  of clearing.
- **10:09** — This is the Saturday queueing-delay problem, except worse than
  theoretical: every task stuck behind the stall is now also paying full
  40ms quantum slices while it waits, on top of whatever the Windpipe block
  itself costs. p99 task latency on the canary shard climbs past 4 seconds.
  Not a SEV-3 kind of number.
- **10:14** — Paged Priya in directly, she was already looking based on the
  same alert. Abhi notified via the incident channel, asked for updates
  every 20 minutes.
- **10:20–11:05** — Mitigation window. Full detail in the separate
  mitigation notes doc, short version: throttled the backfill batch rate,
  then rolled the canary flag back to 0% once the shard drained, which
  stopped new writers from entering block state and let the backlog clear
  on its own.
- **11:12** — Canary shard task latency back under 100ms p99. Lag on the
  audit sink back under 30 seconds. Declared recovered, kept monitoring.
- **11:40** — Formally closed. Total customer-visible degraded window: 90
  minutes (09:42–11:12), affecting only tasks scheduled on the one prod
  canary shard — roughly 4% of total prod task volume for that window.

Wrote "block, don't drop" on Friday like it was the whole story. Turns out
it's the first half of the story. The second half is what happens to
everything sitting behind the block, and I didn't model that part at all.
