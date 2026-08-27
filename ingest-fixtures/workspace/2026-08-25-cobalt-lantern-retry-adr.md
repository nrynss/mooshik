# ADR-014: Retry policy for Cobalt Lantern upstream fetches

**Status:** Accepted
**Date:** 2026-08-25
**Author:** Neom
**Reviewers:** Priya (SRE sign-off pending), Abhi (informed)

## Context

Cobalt Lantern pulls weather observation data from three upstream providers on a rotating schedule. All three have degraded at some point in the last month — one with outright 5xx bursts, one with a habit of silently timing out around the 8-second mark, one with intermittent malformed payloads that fail our schema check. Until now the fetch client retried with a fixed 2-second delay, no jitter, up to five attempts. Two problems with that:

- Fixed delay means every failed fetch across all Cobalt Lantern instances retries in lockstep, which turns a brief upstream blip into a synchronized retry storm.
- Five attempts at a flat 2s delay is 10 seconds minimum before giving up, which is longer than some of our own downstream consumers (the alerting pipeline) are willing to wait for a "no data this cycle" decision.

## Decision

Cobalt Lantern retries failed fetches three times with jitter. Concretely: base delay 500ms, exponential factor 2x, full jitter (delay = random between 0 and calculated max), capped at 4s per attempt. Three attempts total, so worst case before giving up is under 9 seconds but the common case (jittered, first retry succeeding) resolves in under 1.5s.

Three attempts instead of five because our own data showed attempt 4 and 5 almost never succeeded when attempts 1-3 hadn't — checked two weeks of fetch logs, upstream failures that survive three tries are recovering on a timescale of minutes, not seconds, so a fourth or fifth immediate retry was just wasted latency before falling back to last-known-good.

## Alternatives considered

- **Circuit breaker instead of retry-with-backoff.** Rejected for now, not because it's wrong but because it's a bigger change — needs a shared failure-state store across Cobalt Lantern instances, and we don't have consensus on whether that lives in Cloud SQL or somewhere else. Worth revisiting once we have more than three upstream providers.
- **No jitter, just longer fixed delay.** Rejected — doesn't solve the synchronized-retry-storm problem, just spaces the storms out further.
- **Per-provider retry tuning.** Considered, but the three providers don't currently have different-enough failure signatures to justify the complexity. Revisit if the malformed-payload provider keeps being an outlier.

## Consequences

- Downstream consumers (alerting pipeline, dashboard) see "no fresh data" sooner in the worst case, which is the point.
- Slightly more total retry attempts across the fleet during a real outage window is possible if jitter happens to cluster, but the full-jitter algorithm makes that low probability.
- Fetch client code gets marginally more complex (needs a PRNG and an exponential calc instead of a constant). Not a real cost.
- Need to update the runbook Priya's team uses for "Cobalt Lantern data is stale" alerts, since the timing assumptions in there reference the old fixed-delay behavior.

## Rollout

Feature-flagged behind `cobalt_lantern.retry_policy_v2`, default off. Plan is to enable on one instance first, watch fetch success rate and p99 latency for 24 hours against the fixed-delay baseline on the other instances, then roll out fleet-wide if nothing looks wrong. Priya wants the flag kept around for at least a week after full rollout in case we need to roll back fast during an actual upstream incident rather than debugging a new retry policy under fire.

## Open question

Whether to also apply this same policy to the Windpipe writer retries used internally by the retry-dispatch reader — structurally similar problem, different subsystem. Not deciding that here; separate ADR if it comes to that.
