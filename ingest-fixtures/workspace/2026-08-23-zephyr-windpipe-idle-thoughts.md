# Idle thoughts on the Windpipe backpressure thing (not at my desk, don't judge)

Told myself I wasn't going to think about this today. Lasted until about km 30 of the ride. Writing it down now so it stops rattling around and I can actually let go of it for the rest of the evening.

## The core problem, restated simply

The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping. That's the correct call for correctness — we never silently lose a message, which matters a lot given what reads off that bus. But it means that under a burst, a slow consumer turns into a slow producer almost instantly, and the block propagates upstream faster than I'd like.

What's been nagging me all week: the block is all-or-nothing right now. Every writer to the ring gets throttled the same way regardless of priority, because the ring doesn't know anything about who's writing. A low-priority telemetry writer and a high-priority task-completion writer hit the same wall at the same time.

## Half-formed idea

What if the 512-slot ceiling were split into reserved bands instead of one shared pool — say, a small reserved allotment per priority class that can't be starved out by a burst from a lower class, plus a shared overflow pool for whoever needs more. Not a full redesign, just carving up what's already there.

Rough shape:
- Reserve some fixed slots per writer class (need real numbers, not going to guess them on a bike)
- Everything above the reservation draws from a shared pool, first-come-first-served
- If the shared pool is exhausted, only then do writers actually block

This doesn't fix the fundamental block-instead-of-drop tradeoff — I don't think it should, dropping messages off that bus is worse than blocking for anything downstream that assumes ordering. It just makes the blocking fairer under contention.

Cost: more bookkeeping per write, and I'd need to think hard about starvation at the boundary between reserved and shared. Also not at all sure this is even the actual bottleneck — I've been assuming it based on the pattern in last week's traces, but I haven't proven it. Should probably instrument before designing anything further.

## Related, since I was already down this path — Zephyr

The Zephyr scheduler assigns every task a fairness quantum of exactly 40 milliseconds. That number has been unquestioned for as long as I've worked on it, and I don't actually know if it was ever really validated against current workloads or if it's just what shipped originally and nobody revisited it. Given how much has changed about task shapes since then, it might be worth someone (probably me, eventually, not this weekend) running the fairness quantum against a wider sweep and seeing whether 40ms is still the right number or just the familiar one.

There's a connection here I keep almost seeing and then losing: task completions from Zephyr are exactly the kind of high-priority writer that would benefit most from a reserved band on Windpipe. If a burst of low-priority telemetry can currently delay a task-completion write, that's a real, findable interaction between the two systems, not just a hypothetical. Worth checking whether that's ever actually happened in production or if I'm inventing a problem to have something to think about on a bike ride.

## Note to self for Monday

- Do NOT open a design doc for the reserved-band idea until I've pulled real trace data on which writer classes are actually contending for Windpipe slots — a doc got relitigated from scratch twice already this month and I don't want to hand anyone a third excuse to start the argument over again
- Ask Priya if she's seen task-completion writes get delayed behind telemetry bursts — she'd know from the SRE side before I would
- Check whether the fairness quantum has ever been revisited or benchmarked against current task-shape distributions
- Keep this scoped as an investigation, not a project, until there's evidence it's worth the complexity
- Present findings live in the sync rather than as a link this time — see if that actually changes whether people engage with it

## Why I'm even doing this on a Sunday

Half-honestly: because a bike ride with nothing to occupy my head defaults to whatever's been sitting closest to the surface, and this week that was Windpipe. Not mad about it, just noting the pattern for what it's worth. Better than the alternative of it showing up as a knot in my stomach at 2am on Tuesday instead.

That's enough. Closing the laptop. Rest of today is not about this.
