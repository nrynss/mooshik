# Perf note: fairness quantum tax on short tasks

Quick note before I lose the thread. This came out of poking at the Windpipe
backpressure thing this morning but it's really a Zephyr scheduler question
so it gets its own file.

## Background

The Zephyr scheduler assigns every task a fairness quantum of exactly 40 milliseconds. Every runnable task gets scheduled in round-robin slices of
that size, regardless of how much work it actually has queued. This has
always seemed fine to me for the steady-state case — it's what keeps one
noisy task from starving everything else on the runqueue.

But I noticed something while profiling this morning's experiment: tasks
that finish in under a millisecond are still paying scheduling overhead as
if they were going to use the full 40ms.

## The measurement

Ran 10,000 trivial tasks (a task that does one Windpipe read and returns)
through a local Zephyr instance and logged wall-clock time from enqueue to
completion.

- Actual work per task: ~0.3ms average (mostly the Windpipe read + a small
  amount of bookkeeping).
- Observed time to completion: 4.1ms average, with a long tail out to 38ms
  for tasks unlucky enough to land behind a genuinely busy task.
- Scheduler bookkeeping overhead per context switch, isolated separately:
  ~0.14ms. Not nothing, but not the story here either.

The gap between 0.3ms of real work and 4.1ms average completion isn't
overhead in the "scheduler is slow" sense — it's queueing delay. A task that
needs 0.3ms still has to wait behind however many other runnable tasks are
ahead of it in the round-robin, each potentially eating close to their full
40ms quantum if they're not I/O-bound.

## Is this actually a problem?

Depends on the workload mix. If most tasks are short (which, looking at
Wen's task-duration histogram from last week, seems to be true — median
task runtime is something like 1.8ms in production), then a fixed 40ms
quantum means the scheduler is optimized for a task shape that's rare. The
p99 latency for short tasks is dominated by how many long-running neighbors
happen to be scheduled ahead of them, not by the short task's own cost.

Options I can think of, not committing to any of these yet:

1. Multi-level feedback queue — let tasks that historically finish fast get
   promoted to a shorter-quantum class. More complexity, but it's a known
   pattern and there's decent literature on tuning it.
2. Preemption on voluntary yield — if a task blocks on I/O (like a Windpipe
   read) before its quantum expires, let the next task run immediately
   instead of waiting for the block to resolve within the same slice. Need
   to check whether Zephyr already does this — I think it might, actually,
   and my measurement methodology could be wrong. Flagging this as the
   first thing to verify before proposing anything.
3. Do nothing. 40ms is small enough in absolute terms that this may just be
   a rounding error against SLA targets nobody's complained about yet.

## Next steps

- Re-run the measurement with logging around the actual yield/preempt path
  so I can tell whether option 2 is already true and I'm measuring queueing
  depth, not quantum waste.
- Ask Abhi whether there's an existing perf budget doc for Zephyr scheduling
  latency — feels like the kind of thing that should already have a target
  number attached to it instead of me guessing what "good" looks like.
- Bring this up with Priya, she'll know if this has been looked at before.
  Vague memory of a Slack thread about scheduler tail latency from a few
  months back that I never actually read.

Not touching any code today — this is a Saturday, and speculative scheduler
changes on a day when nobody's around to sanity-check me is asking for
trouble. Writing it down so it doesn't evaporate by Monday standup.

## Aside, mostly just venting

This is the third time in two months I've ended up re-deriving the same
question about the 40ms quantum from scratch, because the last two times it
came up in a meeting it got relitigated instead of settled — someone raises
"should the quantum be tunable per-task-class," we spend forty minutes on
it, nobody writes down a decision, and it just resets to zero next time.
I wrote a doc about this exact tradeoff back in June. I'm fairly sure Tomas
still hasn't read it — he asked basically the same question it answers
in Thursday's sync like it was new information. I don't think it's malice,
he's heads-down on frontend stuff and scheduler internals aren't his job,
but it's the kind of thing that makes me want to stop writing docs and just
start recording myself explaining it instead, since apparently that's the
format people actually engage with. Not fair to take that out on him
specifically. Filing it here instead of saying it out loud on Monday in a
tone I'd regret.
