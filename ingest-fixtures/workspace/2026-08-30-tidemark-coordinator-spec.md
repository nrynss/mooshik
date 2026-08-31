# Technical Specification: Tidemark Coordinator Interface

**Date:** 2026-08-30 14:00  
**Author:** Neom  
**Component:** `tidemark-coordinator`  

## Core Data Structures & Interface

```go
type FencingToken int64

type LeaseGrant struct {
    PartitionID   string       `json:"partition_id"`
    ConsumerID    string       `json:"consumer_id"`
    FencingToken  FencingToken `json:"fencing_token"`
    LeaseDuration time.Duration `json:"lease_duration"` // 15s heartbeat lease
    ExpiresAt     time.Time    `json:"expires_at"`
}

type HeartbeatResponse struct {
    Acknowledged bool         `json:"acknowledged"`
    NewExpiresAt time.Time    `json:"new_expires_at"`
    FencingToken FencingToken `json:"fencing_token"`
}

type Coordinator interface {
    // AcquireLease attempts to claim a partition for a consumer.
    AcquireLease(ctx context.Context, partitionID, consumerID string) (*LeaseGrant, error)

    // RenewHeartbeat extends an active 15s heartbeat lease.
    RenewHeartbeat(ctx context.Context, partitionID, consumerID string, token FencingToken) (*HeartbeatResponse, error)

    // ReleaseLease gracefully yields partition ownership.
    ReleaseLease(ctx context.Context, partitionID, consumerID string, token FencingToken) error
}
```

## Protocol Invariants
- Leases are exclusive per `PartitionID`.
- Coordinator increments `FencingToken` on every fresh lease grant.
- Consumer heartbeat renewal runs on a proactive 5s ticker.
