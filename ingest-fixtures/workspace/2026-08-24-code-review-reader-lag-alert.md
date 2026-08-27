# Code review — reader-lag alert (Priya's PR)

PR: `windpipe-reader-lag-alert`, Priya, opened Friday afternoon, this is the
follow-up from the retry-storm postmortem. Reviewing async through the
afternoon between other things, notes pasted here as I go rather than
losing them in the PR tool.

## Summary of the change

Adds a background watcher that polls each reader's acked offset against the
current ring tail, and pages if any reader falls more than 2000 messages
behind for more than 60 seconds. Reuses the existing `:9091/debug/windpipe`
occupancy endpoint I was hitting manually over the weekend — good, one less
thing to build.

## Comments

**offset polling interval** — set to 5s. Asked whether that's tight enough
given a wedged reader could, per Friday's decision, block the entire
writer path. Priya's answer: yes, because the alert threshold (60s sustained
lag) is the real gate, not the poll interval — 5s just needs to be faster
than 60s by a comfortable margin. Fair, approved that part without more
back and forth.

**paging integration** — this is the one that needed real discussion. The
watcher needs a credential to call the paging API. First draft had it
reading from an env var set directly in the Zephyr deploy config, which
also happens to be the same config object that feeds the task dependency
graph the scheduler introspects for debugging. Flagged this — anything that
lands in that config object is queryable by anyone who can read the graph,
which is a much wider audience than "people who should have paging creds."

Left this as the review comment, pulling it in here too because it's worth
having written down somewhere more durable than a PR thread that'll get
squashed:

> Secrets never enter the graph: the vault is the only place a credential value lives.

Asked Priya to switch to a vault reference (an opaque ID the watcher
resolves at call time) instead of the raw value in config. She'd actually
already run into this exact pattern for the audit sink's Cloud SQL creds
and just forgot to apply it here — not a knowledge gap, just a Friday-brain
copy-paste from an older watcher template. Fixed in the next push, took
about ten minutes.

**alert fatigue question** — Tomas commented asking whether this affects
the frontend live-task view. It does not. Third time in four days he's
asked some version of this question across three different threads, and I
get that the churn on that view has made him twitchy about anything
touching Windpipe, but I did already answer this exact question in standup
this morning and again on Friday's design doc, which he was cc'd on. Gave
the short answer again, moved on. Not going to make it a whole thing in a
PR comment, but noting it here because it's the third time and at some
point I'm going to just link him the doc instead of re-typing it.

## Verdict

Approved pending the vault-reference fix. Priya pushed it about 20 minutes
after my comment, re-reviewed, looks right — watcher resolves the paging
credential by ID at call time, nothing sensitive sits in the deploy config
or anywhere the task graph can see it. Merging once CI is green, which,
given this morning, might take longer than usual.
