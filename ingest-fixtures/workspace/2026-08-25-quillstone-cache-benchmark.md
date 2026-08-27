# Quillstone cache — cold vs warm benchmark, Tuesday run

Tomas asked last week why frontend CI felt slower since the monorepo split, and I said I'd actually measure it instead of guessing. Ran this at lunch while the Windpipe patch was in review.

## Setup

The Quillstone build cache lives on the shared NAS under /srv/quillstone. Every CI runner mounts it read-write over NFS; local dev machines mount it read-only and push up only from the nightly job. Three scenarios:

1. **Cold** — cache dir emptied, full build from scratch.
2. **Warm, local** — cache populated from a build five minutes earlier on the same runner.
3. **Warm, NAS** — cache populated by a *different* runner's build, pulled fresh over NFS.

Used `hyperfine` with 5 runs each, 1 warmup, on the `zephyr-scheduler` and `cobalt-lantern` build targets since those are the two Tomas and I both touch regularly.

## Results

```
zephyr-scheduler:
  cold:        142.8s ± 3.1s
  warm (local): 18.4s ± 0.9s   (7.8x)
  warm (NAS):   26.1s ± 1.6s   (5.5x)

cobalt-lantern:
  cold:        61.2s ± 2.0s
  warm (local): 9.7s ± 0.4s    (6.3x)
  warm (NAS):   14.9s ± 1.1s   (4.1x)
```

The NAS-warm case is consistently 40-50% slower than local-warm, which lines up with what Tomas was feeling — most frontend CI runners don't get scheduled back onto the same box twice in a row, so in practice frontend builds are almost always hitting the NAS path, not the local-disk path.

## Where the NAS overhead actually goes

Broke down the NAS-warm `zephyr-scheduler` run with `strace -c` on the cache-read phase:

- 61% — NFS `read` syscalls on cache object files (many small files, ~4-40KB each)
- 22% — `stat` calls checking cache manifest freshness
- 17% — everything else (decompression, linking)

The `stat` overhead is the interesting one. Quillstone's manifest check does a `stat` per cache entry to compare mtimes before trusting a hit, and over NFS each `stat` is a round trip. `zephyr-scheduler` has 1,340 cache entries; that's 1,340 round trips just to validate the cache before a single byte of the useful data gets read.

## What I'd change

- Batch the manifest freshness check into one directory listing instead of one `stat` per entry — should collapse most of that 22% into a handful of round trips.
- Consider a small local disk cache in front of the NAS mount for CI runners specifically (not dev machines, they're fine with NAS-warm), so a runner reused within, say, 20 minutes gets local-warm speed.

Neither is a today thing. Filed both as follow-ups, tagged Tomas since the batched-manifest change touches the same code path his team's asset-fingerprinting job walks.

## Raw numbers, for the record

| target | cold | warm/local | warm/NAS | speedup local | speedup NAS |
|---|---|---|---|---|---|
| zephyr-scheduler | 142.8s | 18.4s | 26.1s | 7.8x | 5.5x |
| cobalt-lantern | 61.2s | 9.7s | 14.9s | 6.3x | 4.1x |

Good enough to close the loop with Tomas on Slack — sent him the table, not the full strace breakdown, he doesn't need that much detail.
