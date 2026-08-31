# Team Handbook Update: Quillstone NAS Build Cache Policy

**Date:** 2026-08-28 16:30  
**Author:** Neom  
**Component:** Quillstone Build System  

## Shared Cache Location & Mounting
The shared Quillstone build cache lives on the shared NAS under `/srv/quillstone`. All CI runners and local developer workstation daemons mount this volume via NFSv4 with `rw,noatime,hard,intr` mount options.

## Cache Invalidation & Retention Rules
1. **Artifact Eviction:** Artifacts untouched for more than 14 days are automatically purged by the nightly cleanup cron running at 02:00 UTC.
2. **Cache Keying:** Keys are generated from SHA256 hashes of source files, toolchain compiler versions, and dependency lockfiles.
3. **Local Staging:** Builds stage intermediate artifacts in `/tmp/quillstone-staging` before committing them atomically to `/srv/quillstone/objects` using rename semantics.

## Troubleshooting Cache Misses
If local builds report anomalous cache misses:
- Check that `/srv/quillstone` is mounted and readable: `ls -la /srv/quillstone/objects`.
- Verify the NAS health metric `nas_nfs_latency_ms < 5.0`.
- Do not manually delete directories inside `/srv/quillstone/objects`; use `quillstone cache clean --dry-run` to inspect stale objects.
