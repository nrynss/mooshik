"""CLI entry: `python3 -m ingester --root <corpus> [--dry-run]`."""

from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

from .config import Settings
from mooshik_common.vertex import make_client

from .extraction import ConceptExtractor
from .pipeline import ingest, plan
from .writer import LamboMcpWriter


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python3 -m ingester",
        description=(
            "Mooshik bootstrap ingester: corpus -> Gemini Flash concepts "
            "-> Cloud SQL graph via lambo serve (MCP)."
        ),
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path("."),
        help="corpus root directory (default: cwd)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="walk and scan only; no Vertex calls, no graph writes",
    )
    parser.add_argument(
        "-v", "--verbose", action="store_true", help="debug logging"
    )
    return parser


def _render(report) -> str:
    lines = [
        f"candidates : {report.candidates}",
        f"written    : {report.written}",
        f"resumed    : {report.resumed}",
        f"concepts   : {report.concepts}",
        f"derive calls: {report.derive_calls}  actions: {report.action_calls}"
        f"  chunks: {report.chunks}",
        "dropped (path only — matched content is never logged):",
    ]
    for source, reason in report.dropped:
        lines.append(f"  - {source} [{reason}]")
    if not report.dropped:
        lines.append("  (none)")
    return "\n".join(lines)


async def _async_main() -> int:
    args = _build_parser().parse_args()
    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(levelname)s %(name)s: %(message)s",
    )
    settings = Settings.from_env(args.root, dry_run=args.dry_run)

    if args.dry_run:
        kept, report = plan(settings)
        print(_render(report))
        return 0

    extractor = ConceptExtractor(
        client=make_client(
            project=settings.project,
            location=settings.location,
            credentials_path=settings.credentials_path,
        ),
        model=settings.model,
        sleep_secs=settings.sleep_secs,
        max_attempts=settings.max_attempts,
    )
    async with LamboMcpWriter(settings.lambo_serve) as writer:
        report = await ingest(settings, writer, extractor)
    print(_render(report))
    return 0


def main() -> None:
    try:
        sys.exit(asyncio.run(_async_main()))
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
