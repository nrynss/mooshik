# RFC Draft: Tidemark Stream Watermark Coordinator

**Date:** 2026-08-28 14:00  
**Author:** Neom  
**Target Milestone:** Tidemark v0.1  

## Objective
Provide lightweight, distributed watermark tracking and partition checkpoint coordination for stream consumers without introducing heavy external consensus clusters.

## Architectural Design

### 1. Partition Leases & Heartbeats
- Each stream partition is assigned a single active consumer worker via an ephemeral lease.
- **Heartbeat Lease Window:** Workers acquire a 15s heartbeat lease from the coordinator.
- Consumers renew their lease by sending periodic heartbeat pings. If no heartbeat is received within 15 seconds, the coordinator marks the lease expired and reassigns the partition.

### 2. Fencing Tokens
- To prevent split-brain writes during network partitions or GC pauses, the coordinator issues a monotonically increasing `FencingToken` with each lease grant.
- Storage sinks reject writes carrying a stale `FencingToken`.

### 3. State & Checkpointing
- Checkpoints are committed to the metadata store only when consumers hold a valid lease and acknowledged watermark offsets.

## Open Questions for Monday
- Is the 15s heartbeat lease window sufficient under heavy heap GC pressure?
- Proactive heartbeat interval: should workers ping at 5s (1/3 lease time) to provide safety margin?
