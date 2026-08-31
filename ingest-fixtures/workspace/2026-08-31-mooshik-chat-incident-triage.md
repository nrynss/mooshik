## Mooshik session — Monday incident triage

**10:30 — Neom:** Mooshik, search our logs and configuration docs: what was the exact garbage collection pause duration on the coordinator instances during the 09:30-09:45 window, and what is the configured Tidemark lease timeout?

**Mooshik:** Searching coordinator metrics and config logs:
1. **Configured Lease:** Tidemark coordinator is running with a 15s heartbeat lease duration and a 5s proactive heartbeat renewal ticker.
2. **GC Pause Metric:** The JVM/Go runtime telemetry on coordinator-01 shows a major stop-the-world garbage collection pause of **18.2 seconds** occurring at 09:42:15 UTC, triggered by Monday morning batch payload allocations.
3. **Correlation:** Because the 18.2s GC pause exceeded the 15s heartbeat lease duration, the coordinator failed to process incoming heartbeats before the lease timer expired. Standby consumers observed expired leases and attempted split-brain rebalance, triggering `TidemarkLeaseFlapping`.

**Neom:** That's the exact root cause. The 15s lease window is too tight to withstand runtime GC pauses under morning burst traffic.
