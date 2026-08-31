# War Room Timeline & Root Cause Isolation

**Date:** 2026-08-31 11:15  
**Incident:** Tidemark Lease Flapping & Consumer Rebalancing Storm  
**Participants:** Neom, Priya, Wen  

## Incident Timeline
- **09:30:** Tidemark v0.1 enabled on Partitions 0–3.
- **09:42:** Monday morning batch ingest generates 80MB/s allocation burst on coordinator-01.
- **09:42:15:** Runtime executes major GC pause lasting 18.2 seconds.
- **09:42:30:** The 15s heartbeat lease expires while coordinator is paused.
- **09:42:35:** Standby consumers observe lease expiry and claim partition ownership; original consumers wake up from pause and attempt renewal. Monotonic FencingTokens successfully block split-brain writes, but cause frantic lease rebalancing loops (`TidemarkLeaseFlapping`).
- **09:45:** PagerDuty alert fires as consumer lag reaches 45k messages.
- **10:30:** Root cause isolated: 15s lease timeout is too tight for runtime GC pause envelopes.
- **11:00:** Consensus reached on mitigation: bump lease grace period to 45s and implement jittered heartbeat retries.
