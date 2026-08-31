# INCIDENT ALERT: TidemarkLeaseFlapping & Consumer Lag Spike

**Date:** 2026-08-31 09:45  
**Severity:** SEV-2 (Elevated Consumer Lag)  
**Trigger Alert:** `TidemarkLeaseFlapping` / `TidemarkConsumerLagSpike`  

## Incident Notification Details
At 09:45 AM UTC, PagerDuty triggered SEV-2 alert on stream consumer cluster:
- **Metric Alarm:** `TidemarkLeaseFlapping` firing across Partitions 0, 1, 2, 3.
- **Consumer Lag:** Ingestion lag spiked rapidly from 120 messages to 45,000 messages within 8 minutes.
- **Symptom:** Consumer workers repeatedly acquiring and dropping partition leases every 15 to 20 seconds, causing continuous partition rebalancing storms.

## Initial Response
- 09:48 — Neom acknowledged PagerDuty alert and opened war-room channel `#incident-20260831-tidemark`.
- 09:50 — Wen and Priya joined the bridge.
- 09:52 — Paused further deployment to Partitions 4–15 while triaging canary partitions.
