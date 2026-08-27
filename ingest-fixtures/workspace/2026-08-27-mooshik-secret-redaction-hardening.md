# Hardening: keep credentials out of Mooshik's memory graph

Small change, shipped this morning, prompted by something that almost went
wrong yesterday during the Windpipe incident.

## What happened

Around 09:35 yesterday, mid-incident, someone pasted a chunk of the audit
sink's reconnect logs into the incident channel so we could all see the
failure pattern. Those logs, on the reconnect path, sometimes include the
full Cloud SQL connection string the reader is trying (and failing) to use
— including the embedded credential. Mooshik is watching that channel the
way it watches everything I'm in; the ingest pipeline is supposed to catch
that before it ever gets written into the graph, and it did, this time.
But "it worked" isn't the same as "I checked why it worked," so I spent
this morning actually reading the redaction path instead of trusting it by
reputation.

## What I found

The scanner that runs before ingest matches a handful of credential-shaped
patterns — connection strings with embedded auth, bearer-token-looking
values, PEM block headers, a couple of cloud-provider key prefixes. It's
regex-based, sits in front of the graph writer, and anything that matches
gets replaced with a redaction marker before the document is embedded or
linked to anything. The log paste from yesterday matched on the
connection-string pattern and got redacted correctly. Good. But I couldn't
find a test that actually exercises this end to end — the regex has unit
tests, the graph writer has unit tests, but nothing proves the two are
wired together correctly, which is the only property I actually care
about.

## The change

- Added an integration test that ingests a synthetic document containing
  several credential-shaped strings (generated, not real, and deliberately
  malformed so they can't be mistaken for anything live) and asserts none
  of them reach the graph store, in any form — not redacted-and-kept,
  gone entirely.
- Added an explicit invariant to the ingest pipeline's doc comment, because
  I want this to be the kind of thing nobody has to rediscover under
  incident pressure:

  > Secrets never enter the graph: the vault is the only place a credential value lives.

- Tightened one pattern that was too narrow — it only matched connection
  strings with an explicit scheme prefix, which meant a couple of legacy
  DSN formats in the older Zephyr services could have slipped through.
  Widened it, reran the test suite against six months of sampled
  incident-channel history (offline, sandboxed, nothing touched prod),
  zero new matches beyond what was already being caught, so I'm reasonably
  confident this wasn't a live gap, just an untested one.

## Why this matters beyond yesterday

Mooshik holding a lifelong memory of my workspace is only worth trusting
if I never have to wonder whether something sensitive is sitting in it.
Yesterday was a near-miss, not an incident, but near-misses are exactly
the cases worth hardening while they're still cheap to fix. Filed under:
the boring kind of infrastructure work that only pays off the day it
doesn't happen.

## Follow-up

- Want the same integration test pattern applied to the Slack ingest path
  specifically, not just the generic document path — different code, same
  risk. Not today.
- Mentioned to Priya in passing; she wants the redaction marker format
  documented somewhere discoverable instead of living only in my head.
  Fair. Adding to the doc pass I'm doing later today anyway.
