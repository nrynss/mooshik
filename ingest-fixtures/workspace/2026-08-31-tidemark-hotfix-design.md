# Tidemark Hotfix Design: 45s Lease Grace Window & Jittered Renewals

**Date:** 2026-08-31 12:00  
**Author:** Neom  
**Reviewer:** Priya  
**Status:** Approved for Staging  

## Hotfix Architecture & Parameter Changes

1. **Lease Grace Window Expansion:**
   - Increase `LeaseDuration` from 15s to **45s lease grace window**.
   - This provides a 2.5x safety buffer over the maximum observed 18.2s runtime GC pause envelope.

2. **Proactive Jittered Heartbeat Renewals:**
   - Consumers send heartbeat renewal every **5s proactive heartbeat renewal**.
   - If a heartbeat fails or times out, consumers retry with exponential jitter (1s base, 2x multiplier) up to 3 times before lease expiry.

3. **Fencing Token Safety:**
   - Maintain monotonic `FencingToken` checks on all sink writes to guarantee zero data loss during rebalancing.

Priya reviewed and approved the hotfix patch at 12:15. Preparing unit test harness and canary build.
