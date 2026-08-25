# Adversarial review — Mooshik M1/M6, round 9

**Scope:** close P3-R8-1 only. Round 8 was REQUEST_CHANGES on that one P3.
**Date:** 2026-08-25
**Verdict:** **P3-R8-1 closed.** Failed staging pathnames are not removed.

## Closure

Round 8: `remove_staging_directory_if_unchanged` checked `(st_dev, st_ino)` then
`unlinkat(..., AT_REMOVEDIR)`. A same-UID actor could rename the checked
directory away and install an empty replacement at `leaf` in that window.

There is no portable descriptor-bound rmdir. The safe choice is fail-closed:
leave the random `.mooshik-stage-*` directory for later operator cleanup.
`preserve_staging_directory` still opens and identity-checks the pathname so
the window exists in source, then runs an injected barrier, then **does not
unlink**.

Pin: `staging_cleanup_does_not_remove_a_replacement_after_identity_check`.
The barrier swaps in an empty replacement after the identity check; the
replacement remains. Reintroducing `unlinkat` after that barrier fails the pin.

`src/secure_path.rs` is the only code change. P3-R8-1's suggested hook is that
barrier, not a new removal primitive.
