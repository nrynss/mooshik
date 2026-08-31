## 22:00 — Friday Evening Notebook

Sitting in the living room with a mug of chamomile tea. The apartment is quiet.

Reflecting on the week's engineering work:
There is something deeply satisfying about deleting complex workaround code in favor of a clean, simple invariant. We spent days agonizing over dropped messages and reader lag alarms, trying to tune drop-tail queues and synthetic throttling. In the end, two straightforward constraints solved both problems:
1. Windpipe: Hard cap of 512 in-flight messages with blocking backpressure.
2. Cobalt Lantern: 3 retries with full jitter (ADR-014).

When the primitives are honest, the system behaves predictably.

Tidemark is up next for Monday. The 15s heartbeat lease design feels promising for partition coordination, though I want to be very careful about edge cases around JVM and runtime GC pauses.

Closing the laptop for the weekend. Tomorrow morning: fresh Chemex coffee, researching Bali surf spots in Uluwatu, and a 45km northern trail ride.
