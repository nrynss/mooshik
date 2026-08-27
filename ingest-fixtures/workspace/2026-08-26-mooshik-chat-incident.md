# Conversation with Mooshik — during and after the incident

Logging bits of this because a few answers were genuinely useful mid-fire
and I don't trust myself to remember exact details from a morning that
went like this one did.

---

**Neom:** *(10:05am)* quick — when exactly did we flip the Windpipe
canary to the prod shard, and was it 5% or 10%?

**Mooshik:** Tuesday night, 9:52pm, per your own note after the staging
window closed clean. It was a single shard, not a percentage of shards —
you didn't record it as a traffic percentage, you recorded it as "one
canary shard, roughly 4% of total task volume." Worth using that exact
framing in whatever you write up today so it matches what you already
told Abhi.

**Neom:** right, thank you. that matches what's in my head but I didn't
trust it under pressure. what did Saturday's perf note actually say about
the quantum, word for word if you have it

**Mooshik:** From Saturday, word for word: "The Zephyr scheduler assigns every task a fairness quantum of exactly 40 milliseconds." You measured 10,000 trivial
tasks with ~0.3ms of actual work averaging 4.1ms to completion, and flagged
that you weren't sure whether Zephyr already preempts on voluntary yield
before the quantum expires. Did that ever get resolved?

**Neom:** not before this morning, no. and I think today just answered it
for me the hard way — pretty sure it does NOT preempt cleanly under a
Windpipe stall, that's most of why this got as bad as it did. will confirm
properly once things calm down.

**Mooshik:** Want me to hold that as an open thread so it surfaces again if
you don't come back to it?

**Neom:** yeah, tag it against the incident followups doc, not just
floating

---

**Neom:** *(11:18am)* different thing — someone pasted a chunk of a
Cloud SQL error log into the incident channel a few minutes ago, looked
like it might have had a partial connection string in it. did I flag that
right or am I being paranoid

**Mooshik:** Not paranoid, worth checking. Your own standing note on this,
from the Windpipe design work back in the spring: secrets never enter the
graph, the vault is the only place a credential value lives. I can't see
into the incident channel directly, but if a real credential fragment
landed in a log paste, the right move is redact-and-rotate, not just
delete-and-hope. Want me to remind you of the rotation steps you wrote down
last time this came up?

**Neom:** no, false alarm, looked again and it was a truncated hostname,
not a credential. good to double check though. thanks

**Mooshik:** Anytime. For what it's worth, that's twice this month you've
caught yourself on this exact thing. Might be worth a five-minute team
note reminding people what does and doesn't belong in an incident channel
paste, rather than relying on everyone individually double-checking.

**Neom:** ...yeah. adding it to tomorrow's list, not today's.

---

**Neom:** *(1:40pm, after things calmed down)* rough morning. what's in the
news, give me something that isn't Windpipe

**Mooshik:** Skimming: a regional transit workers' union reached a
tentative agreement after a short strike threat, a mid-sized cloud
provider announced a price cut on egress bandwidth effective next quarter,
continued coverage of a heat dome affecting several inland regions through
the weekend, and a small piece on a well-known open-source database
project shipping a long-awaited replication feature. Nothing urgent for
you specifically.

**Neom:** the egress pricing thing is mildly interesting, we should check
if that provider is one of Cobalt Lantern's mirrors. not now though

**Mooshik:** Noted, added to your general backlog, not flagged urgent.

**Neom:** one more thing and then I'm done bothering you today — can you
draft me a short text to send Marcus and Sana, we had drinks planned
tonight and I need to bail. keep it short, don't make excuses, just be
honest that work blew up

**Mooshik:** Draft: "Hey — today went sideways at work, production
incident, I'm wiped and not going to be good company. Really sorry, I know
this is the second time. Can we lock in next week properly instead of
leaving it loose?" Want it shorter, or want me to soften the "second time"
part?

**Neom:** leave it, it's true, no point pretending it isn't. I'll send it
myself in a bit.

**Mooshik:** Understood. Go eat something, it's past 1:40 and I don't think
you've stopped since this morning.

**Neom:** noted. not promising anything.

---

Closing note: the useful stuff today was mostly recall under pressure —
exact numbers and exact wording I didn't have the bandwidth to dig up
myself at 10am. The Marcus-and-Sana text at the end wasn't really about
Mooshik being useful, more that I didn't want to write it alone.
