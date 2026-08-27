# Monday standup + week planning

08:52, second coffee already gone. Back from the long weekend and the queue
looks exactly like I left it Friday, which is either good discipline or
nobody else touched anything. Bike commute was fine, cool enough that I
didn't need to change a shirt at the rack for once.

## Standup — 9:15am

**Yesterday / over the weekend**
- Ran the Windpipe burst-load experiment at home Saturday — confirmed the
  ring blocks producers instead of dropping at 512 occupancy, matches the
  decision record from Friday. Numbers written up separately.
- Nothing shipped, this was reading and testing, not code.

**Today**
- Land the Windpipe overflow-block change behind a no-op flag (per Friday's
  plan — flip it in staging tomorrow, prod after 24h of clean reader-lag
  dashboards).
- 11:00 design review on the Cobalt Lantern ingest pipeline with Wen — the
  forecast-provider latency spikes she flagged last Tuesday.
- Review Priya's PR for the reader-lag alert (the ticket from Friday's
  postmortem follow-up).
- 2:00 1:1 with Abhi.
- Finally reply to Priya about the on-call rotation — sat in my notifications
  all weekend, need to just answer it.

**Blockers**
- None yet. It's 9:15. Ask me at noon.

Priya: audit sink v2 PR up for review, wants eyes before end of day so she
can start the alert wiring tomorrow. Tomas: frontend live-task view is
unaffected by the Windpipe change (confirmed again, he keeps asking, fair
enough given how much churn that view has had). Wen: Cobalt Lantern
ingest p99 has been climbing for eight days straight, wants to walk through
options before it gets worse.

## This week — priorities

1. Windpipe overflow-block rollout — flag today, staging Tuesday, prod
   Wednesday if lag dashboards are clean. This is the thing that actually
   matters this week, everything else is secondary to not blocking it.
2. Reader-lag alert (Priya) — review today, merge by Wednesday so it's live
   before we flip the flag in prod. Would rather have the alarm before the
   thing it's alarming about.
3. Cobalt Lantern ingest latency — scope today in the design review, decide
   whether it's a Thursday fix or a "file it and move on" thing. Eight days
   of climbing p99 is not urgent-urgent but it's not nothing either.
4. On-call rotation — reply to Priya, figure out whether I'm on for the
   September block. Overdue since Friday, self-inflicted.
5. Quillstone — nothing planned, but the cache has felt slow to warm on
   clean checkouts twice last week. Not investigating unless it gets worse.
   (Famous last words, we'll see.)

## Notes to self

- Don't let the Cobalt Lantern design review turn into a full redesign
  meeting. Wen tends to want to solve everything in the room. Scope it to
  "is this the provider or is this us."
- Abhi's 1:1 — he'll ask about the September roadmap doc, I haven't started
  it. Just say so, don't pretend otherwise.
- If today goes sideways before lunch, it wouldn't be the first Monday.
