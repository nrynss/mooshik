# Windpipe overflow — Tuesday debugging log

## 08:10 — starting late

Standup ran long (Abhi wanted a status readout on the Q3 scheduler roadmap before the offsite), so I didn't sit down properly until 8:10. First thing in the queue: an overnight flag from Priya. She was on call and saw three `windpipe_writer_blocked_total` spikes between 02:14 and 04:51, each lasting 40-90 seconds. Nothing paged — under threshold — but she left a note in #zephyr-oncall asking me to look when I got in, since I touched the writer path last week.

## 08:25 — reproducing

Pulled the Cloud SQL flush logs for the affected window. Windpipe persists the ring to Cloud SQL every 250ms, so I expected three batches of oversized writes lining up with the spikes. Instead I found gaps — the flush interval itself stretched to 800ms, 1.1s, and 640ms in those three windows. That's not a flush-side problem, that's something upstream stalling the writer.

Checked `zephyrd` goroutine counts across the same window (we sample every 10s into the metrics DB): steady at ~340 baseline, jumped to 1,180 right before the first spike at 02:14:07, then settled back down by 02:15:40. Classic pileup shape — something is queuing work faster than it drains.

## 09:05 — Priya joins

Pinged Priya once she was online properly (she'd been half-watching from her phone). We hopped on a call. She pulled the Grafana panel for consumer lag on the three Windpipe readers — the retry-dispatch reader, the audit-log reader, and the metrics-sampler reader. Audit-log reader lag jumped to 480 messages right at 02:14, just under the ring ceiling. That's the smoking gun.

Re-read the Windpipe spec doc together on the call to double check the invariant, because I always second-guess myself on this one: "The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping." So at 480 in-flight we're not overflowing yet, but we're close enough that any writer with a slightly bigger batch pushes past 512 and blocks. That block is exactly the writer stall we're seeing in the flush gaps.

## 09:40 — why was the audit-log reader lagging

Checked what the audit-log reader was doing at 02:14. It batches writes to the audit table in groups of 64, and if a batch fails it retries the whole batch rather than just the failed rows. Wen had shipped a schema migration on the audit table Sunday night (added a nullable `source_region` column) and the migration's `ALTER TABLE` was still holding a brief lock during a compaction job that ran at 02:00 sharp — cron, not deploy-triggered. Every batch write from 02:00 to roughly 02:13 queued behind that lock, and once it cleared, the reader had a huge backlog to drain, which is what pushed lag up to 480.

Told Wen about the interaction on Slack — not blaming the migration, just noting the compaction job overlap is worth flagging to Priya's team since it's a recurring weekly cron.

## 10:30 — is this actually a problem

Debated with Priya whether this needs a fix or just a runbook note. Arguments for "just document it": the writer *blocking* instead of dropping is the system working as designed — no data loss, just backpressure. Arguments for "fix it": three occurrences before 5am on a Tuesday means it'll happen every week at the same time as the compaction cron, and eventually the queue will be long enough that a real writer (the retry-dispatch reader, which is latency sensitive) gets caught behind it too.

Landed on: fix the audit-log reader's batch-failure handling so it retries only the failed rows within a batch, not the whole batch. That should keep queue growth roughly linear instead of the current all-or-nothing retry amplifying it. Also asked Priya to move the weekly compaction cron 15 minutes earlier so it doesn't overlap with the 02:00-02:15 low-traffic window where the audit batches concentrate — turns out that window exists because of a different cron (log rotation) that's been there since before either of us joined the team.

## 11:00 — the ring-size conversation, again

Between the patch and lunch, got pulled into a thread in #zephyr-eng where someone (not naming names in my own notes, but it's the third time this quarter) reopened "should we just bump the Windpipe ring past 512" as if it hadn't been discussed. I wrote up the tradeoffs in the spec doc back in June — larger ring means slower Cloud SQL flush batches means worse recovery time on restart, it's not free — and I said so again today, more tersely than I meant to. Genuinely tired of re-litigating a decision that's already written down because nobody reads the doc before starting the thread. Closed my laptop lid for two minutes just to not respond immediately.

Also: this is the third meeting-shaped interruption today on what was supposed to be a heads-down day — the standup overrun, an unplanned 25-minute call with Priya that could've been async, and now this thread. I like being useful but today has been more triage than build.

## 12:15 — patch drafted

Wrote the partial-retry logic for the audit-log reader. About 40 lines, touches `internal/windpipe/readers/audit.go`. Ran it against a replay of last night's Cloud SQL flush logs locally — queue growth during the simulated lock window stayed under 120 messages instead of climbing to 480. Left it for Priya to review after lunch; she wants to walk through it live rather than just leaving PR comments, which is fine, I'd rather catch anything weird before it merges.

## Notes to self

- The 512 ceiling is genuinely tight for three readers sharing one ring. Worth a separate conversation about whether metrics-sampler should get its own ring instead of sharing Windpipe's.
- Compaction-cron-overlaps-migration-lock is the kind of thing that should be in a runbook, not just in my head and Priya's head.
- Ask Abhi whether the Q3 roadmap slot for "Windpipe ring resize" is worth pulling forward given this.
