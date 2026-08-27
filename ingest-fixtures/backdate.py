#!/usr/bin/env python3
"""Stamp dated fixture documents with the mtime their filename claims.

The ingester reads a file document's historical `event_time` from its mtime
(`walker._file_event_time`), and Lambo's solo promotion policy counts
recurrence over event time with a 24-hour separation gap. A corpus whose
files all share one mtime therefore has no recurrence at all, however many
times a fact repeats inside it — which is exactly the pathology M9 measured
on the first bootstrap graph.

Checking the corpus into git does not preserve the dates: a clone or a
docker build context rewrites every mtime to the checkout time. So the dates
live in the *filenames* (`YYYY-MM-DD-<slug>.md`), which git does preserve,
and this script replays them onto the filesystem. Run it after checkout and
before ingesting — the image does so at build time.

Every file is stamped at the same clock time, so consecutive days land
exactly `SESSION_SEPARATION` apart and each counts as its own session.
Spreading the hours around would look more lifelike and quietly break that:
an 18:00 note followed by an 09:00 one is 15 hours apart and collapses into
a single session.

Usage:
    python3 backdate.py <corpus-root> [--hour 12] [--check]

    --check  report what would change and exit non-zero if anything would,
             without writing. For CI.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

#: Filenames carry the document's date: `2026-08-21-standup.md`.
DATED = re.compile(r"^(\d{4})-(\d{2})-(\d{2})-")

#: Extensions the ingester actually walks (`Settings.extensions`). Anything
#: else is reported rather than stamped, so a corpus file that will never be
#: ingested cannot look dated and covered.
INGESTED = {".md", ".markdown", ".txt", ".rst"}


def dated_files(root: Path) -> list[tuple[Path, datetime]]:
    """Every ingestable file under `root` whose name carries a date."""
    found: list[tuple[Path, datetime]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        match = DATED.match(path.name)
        if not match:
            continue
        if path.suffix.lower() not in INGESTED:
            print(f"  skip (not an ingested extension): {path.name}")
            continue
        year, month, day = (int(part) for part in match.groups())
        found.append((path, datetime(year, month, day, tzinfo=timezone.utc)))
    return found


def stamp(root: Path, hour: int, check: bool) -> int:
    entries = dated_files(root)
    if not entries:
        print(f"error: no dated documents under {root}", file=sys.stderr)
        return 1

    drift = 0
    for path, day in entries:
        want = day.replace(hour=hour).timestamp()
        have = path.stat().st_mtime
        if abs(have - want) < 1.0:
            continue
        drift += 1
        if check:
            print(f"  would stamp {path.name} -> {day.date()} {hour:02d}:00Z")
        else:
            os.utime(path, (want, want))

    days = sorted({day.date() for _, day in entries})
    print(f"{len(entries)} dated documents across {len(days)} days: {days[0]} .. {days[-1]}")
    if check:
        print(f"{drift} would change")
        return 1 if drift else 0
    print(f"{drift} stamped, {len(entries) - drift} already correct")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--hour", type=int, default=12)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if not args.root.is_dir():
        print(f"error: {args.root} is not a directory", file=sys.stderr)
        return 1
    if not 0 <= args.hour <= 23:
        print("error: --hour must be 0..23", file=sys.stderr)
        return 1
    return stamp(args.root, args.hour, args.check)


if __name__ == "__main__":
    raise SystemExit(main())
