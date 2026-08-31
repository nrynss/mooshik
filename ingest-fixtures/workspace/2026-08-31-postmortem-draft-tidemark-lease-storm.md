# Postmortem: Tidemark Lease Flapping & Consumer Lag Incident

**Date:** 2026-08-31 15:45  
**Incident Date:** 2026-08-31 (09:42 – 14:30 UTC)  
**Authors:** Neom, Wen  
**Status:** Complete  

## Executive Summary
On Monday morning at 09:42 UTC, the rollout of Tidemark v0.1 on Partitions 0–3 encountered lease flapping. An 18.2s runtime GC pause on coordinator-01 exceeded the aggressive 15s heartbeat lease timeout, causing standby consumers to trigger split-brain rebalance storms. Consumer lag peaked at 45,000 messages. The incident was mitigated by deploying hotfix PR #172 (bumping to 45s lease grace window with 5s proactive heartbeat renewal). Consumer lag drained to 0 with zero data loss.

## 5 Whys Analysis
1. *Why did consumer lag spike?* Partition leases were repeatedly revoked and rebalanced.
2. *Why were leases revoked?* Coordinator heartbeat timer expired.
3. *Why did heartbeats expire?* Coordinator suffered an 18.2s runtime GC pause under morning allocation load.
4. *Why did the GC pause exceed the lease?* The original 15s heartbeat lease was designed under idealized microbenchmark conditions without accounting for peak heap GC envelopes.
5. *Why was there no data loss?* Monotonic FencingTokens rejected stale out-of-order writes.

## Action Items
- [x] Bump lease timeout to 45s lease grace window (Neom, Done).
- [x] Implement 5s proactive heartbeat renewal with jitter (Neom, Done).
- [ ] Add Prometheus alert for `CoordinatorGC_Pause_Duration > 10s` (Wen, Sept 4).
- [ ] Profile heap allocations during morning batch ingress to reduce GC pressure (Priya, Sept 8).
