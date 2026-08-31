# ADR-014: Cobalt Lantern Retry Policy with Full Jitter

**Date:** 2026-08-28 11:30  
**Author:** Neom  
**Reviewer:** Priya  
**Status:** Accepted  

## Decision & Context

Cobalt Lantern fetches external upstream dataset fragments across intermittent network boundaries. Previous unbounded retries with static backoff caused thundering herd storms on downstream storage during transient outages.

Under ADR-014, Cobalt Lantern implements:
1. **Attempt Limit:** Exactly 3 retries with full jitter before failing to the error queue.
2. **Backoff Parameters:** Base delay of 500ms, exponential multiplier 2x, maximum backoff ceiling of 4s per attempt.
3. **Jitter Formula:** `sleep_duration = random_between(0, min(max_delay, base_delay * 2^attempt))`.

## Review Sign-off Note (Priya — 11:25)
> *"Reviewed the staging metrics and the ADR draft. The reader lag spikes have completely flattened, and the 3-attempt limit with full jitter prevents storage thrashing. Approved to ship into production release."*

ADR-014 is officially accepted. The retry handler code is merged on `main`.
