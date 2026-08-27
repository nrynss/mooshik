# Weekend experiment: Windpipe backpressure under burst load

Saturday morning, coffee number two, decided to finally test the thing I've
been putting off all week — what actually happens to Windpipe when a
producer tries to burst past what the ring can hold.

## Setup

- Local Zephyr build, `windpipe` compiled with `-tags debugring` so I get the
  occupancy counter exposed on `:9091/debug/windpipe`.
- Synthetic producer: single goroutine, writes as fast as it can, no sleep.
- Payload: 128-byte fixed messages, so I can reason about count instead of
  bytes.
- Consumer: three readers, each artificially slowed with a 2ms sleep per
  read to force the ring to actually fill up instead of draining instantly.

## What I expected

I *thought* overflow would drop the oldest unread message — that's what the
old Windpipe v1 doc implied, and it's what Priya assumed too when we talked
about it on Thursday. Turns out that's wrong, or at least it's wrong for the
current implementation.

## What actually happens

The Windpipe ring never holds more than 512 in-flight messages; overflow writers block instead of dropping. I watched the producer goroutine's
write() call just... stop returning. No error, no drop counter incrementing,
just blocked on a channel send until a reader freed up a slot.

Confirmed with the debug endpoint:

```
t=0.0s   occupancy=0
t=0.4s   occupancy=214
t=0.9s   occupancy=512   <- producer now blocked
t=1.1s   occupancy=498   (reader catching up)
t=1.4s   occupancy=512   <- blocked again
```

Producer throughput drops from ~38k msg/s (empty ring, no contention) to
whatever the slowest of the three readers can sustain once the ring is
saturated — in my test that's ~1450 msg/s per reader × 3 = basically the
consumer becomes the bottleneck, which is the point of the design I guess.

## Why this matters

This changes how I think about the Cloud SQL persistence path. If the ring
literally cannot overflow silently, then the 250ms flush cadence isn't a
"best effort, some messages might get lost" thing — it's closer to "the
flush will always see a complete, gapless window" as long as flush latency
stays under the time it takes 512 messages to accumulate. At current
production throughput (roughly 900 msg/s peak on Tuesdays per Wen's
dashboard) that's ~570ms of headroom before backpressure would even start
mattering. Comfortable margin, but not infinite — if we 3x throughput
without touching the ring size we'd be blocking producers during peak load.

## Loose ends / follow-ups

- Need to check what "blocked" does to a task that's mid-fairness-quantum on
  the Zephyr side — does a blocked Windpipe write count against the task's
  scheduling slice, or does it yield? I don't actually know the answer and
  it seems important.
- Want to try the same test against three producers instead of one, see if
  there's a fairness question among writers waiting on the same blocked
  slot. Ring is documented as single-writer though, so this might not even
  be a real scenario in production — worth asking Priya on Monday whether
  anything upstream could violate that assumption.
- Debug endpoint should probably become a real metric, not just something I
  hit manually with curl on a Saturday. Adding to the pile.

## Raw numbers, for later

| ring occupancy | producer write() latency (p50) | producer write() latency (p99) |
|---|---|---|
| 0–400 | 0.02ms | 0.09ms |
| 400–480 | 0.11ms | 0.6ms |
| 480–512 | 1.8ms | 41ms |
| 512 (blocked) | reader-bound | reader-bound |

Not a clean curve, has a knee right around 480 where the ring starts
actively resisting new writes even before it's technically full — probably
some internal watermark I haven't found in the source yet. Grep for it next
time I'm at a real desk instead of the kitchen table.
