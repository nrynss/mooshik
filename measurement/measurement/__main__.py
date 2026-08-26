"""CLI: `python3 -m measurement {sample,grade,report}`.

    sample   --raw N --canonical N [--rejected N] --out F --seed S [--session S]
    grade    SAMPLE [--grades G] [--template T | --apply T]
    report   SAMPLE [--grades G] [--session S]

Reads the live graph through the Connection seam (DSN from
MOOSHIK_POSTGRES_DSN or --dsn). Works only off what the M7/M8 pipelines
wrote; it never inserts graph rows.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .db import DEFAULT_SESSION, PgConnection, dsn_from_env
from .grade import (
    apply_template,
    grades_path_for,
    grade_interactive,
    load_grades,
    save_grades,
    write_template,
)
from .pools import coverage_overall
from .report import render_report
from .sample import read_jsonl, run_sampling, write_jsonl


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python3 -m measurement",
        description=(
            "M9 measurement harness: sample the live Cloud SQL graph, "
            "hand-grade against sources, report precision with Wilson "
            "intervals."
        ),
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_sample = sub.add_parser("sample", help="seeded draws from the live pools")
    p_sample.add_argument("--raw", type=int, default=10)
    p_sample.add_argument("--canonical", type=int, default=5)
    p_sample.add_argument("--rejected", type=int, default=10)
    p_sample.add_argument("--out", type=Path, default=Path("sample.jsonl"))
    p_sample.add_argument("--seed", type=int, default=42)
    p_sample.add_argument("--session", default=DEFAULT_SESSION)
    p_sample.add_argument("--dsn", default=None)

    p_grade = sub.add_parser("grade", help="verdicts, interactive or via editor")
    p_grade.add_argument("sample", type=Path)
    p_grade.add_argument("--grades", type=Path, default=None)
    p_grade.add_argument("--template", type=Path, default=None,
                         help="write an editable verdict template TSV")
    p_grade.add_argument("--apply", type=Path, default=None,
                         help="apply a filled template TSV and persist")

    p_report = sub.add_parser("report", help="markdown report to stdout")
    p_report.add_argument("sample", type=Path)
    p_report.add_argument("--grades", type=Path, default=None)
    p_report.add_argument("--session", default=DEFAULT_SESSION)
    p_report.add_argument("--dsn", default=None)

    return parser


def _connect(args: argparse.Namespace) -> PgConnection:
    return PgConnection(dsn=args.dsn if getattr(args, "dsn", None) else dsn_from_env())


def _cmd_sample(args: argparse.Namespace) -> int:
    items, sizes = run_sampling(
        _connect(args),
        session=args.session,
        n_raw=args.raw,
        n_canonical=args.canonical,
        n_rejected=args.rejected,
        seed=args.seed,
    )
    write_jsonl(items, args.out)
    print(
        f"sampled {len(items)} of "
        f"(raw={sizes['raw']}, canonical={sizes['canonical']}, "
        f"rejected={sizes['rejected']}) -> {args.out} (seed={args.seed})"
    )
    return 0


def _cmd_grade(args: argparse.Namespace) -> int:
    items = read_jsonl(args.sample)
    grades_file = args.grades or grades_path_for(args.sample)
    grades = load_grades(grades_file)

    if args.template:
        write_template(items, args.template)
        print(f"template -> {args.template}: fill column 2 "
              f"(correct/incorrect/unsure), then --apply {args.template}")
        return 0

    if args.apply:
        applied, skipped = apply_template(args.apply, grades)
        save_grades(grades_file, grades)
        print(f"applied {applied}, skipped {skipped} -> {grades_file}")
        return 0

    done = grade_interactive(items, grades)
    save_grades(grades_file, grades)
    print(f"\ngraded {done} this session; {len(grades)} total -> {grades_file}")
    return 0


def _cmd_report(args: argparse.Namespace) -> int:
    from . import pools

    items = read_jsonl(args.sample)
    grades_file = args.grades or grades_path_for(args.sample)
    grades = load_grades(grades_file)
    conn = _connect(args)
    embedded, total = coverage_overall(conn, args.session)
    sizes = {
        "raw": len(pools.raw_pool(conn, args.session)),
        "canonical": len(pools.canonical_pool(conn, args.session)),
        "rejected": len(pools.rejected_pool(conn, args.session)),
    }
    sys.stdout.write(render_report(items, grades, sizes, embedded, total, args.session))
    return 0


def main(argv: list[str] | None = None) -> None:
    args = _build_parser().parse_args(argv)
    handlers = {
        "sample": _cmd_sample,
        "grade": _cmd_grade,
        "report": _cmd_report,
    }
    try:
        sys.exit(handlers[args.cmd](args))
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
