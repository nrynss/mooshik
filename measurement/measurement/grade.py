"""Human-in-the-loop grading.

Grades live in a jsonl sidecar (default ``<sample-stem>.grades.jsonl``), one
``{"node_id": ..., "verdict": ...}`` per line, keyed by node id so they
survive re-sampling: re-grading merges over whatever is already persisted and
never drops entries for items absent from the current sample.

Two modes:

* **editor/file mode** — ``--template FILE`` writes an editable TSV of
  ``node_id<TAB>verdict<TAB>pool<TAB>content`` rows (verdict blank); fill the
  middle column with correct/incorrect/unsure, then ``--apply FILE`` persists
  it.
* **interactive mode** — default when neither flag is given: prints each
  ungraded item with its source excerpt, reads c/i/u/s verdicts one per item.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from .excerpt import excerpt_for
from .sample import VERDICTS, SampleItem

PROMPT = "verdict [c]orrect / [i]ncorrect / [u]nsure / [s]kip / [q]uit: "
KEYMAP = {"c": "correct", "i": "incorrect", "u": "unsure"}


def grades_path_for(sample_path: Path) -> Path:
    return sample_path.with_name(sample_path.stem + ".grades.jsonl")


def load_grades(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    grades: dict[str, str] = {}
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            rec = json.loads(line)
            verdict = rec["verdict"]
            if verdict not in VERDICTS:
                raise ValueError(f"invalid verdict {verdict!r} for {rec.get('node_id')}")
            grades[rec["node_id"]] = verdict
    return grades


def save_grades(path: Path, grades: dict[str, str]) -> None:
    """Merge into what is already persisted; node-id keying makes this
    idempotent, so re-sampling can never drop earlier verdicts."""
    merged = load_grades(path)
    merged.update(grades)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        for node_id in sorted(merged):
            fh.write(json.dumps({"node_id": node_id, "verdict": merged[node_id]}) + "\n")
    tmp.replace(path)


def write_template(items: list[SampleItem], path: Path) -> None:
    with path.open("w", encoding="utf-8") as fh:
        for item in items:
            content = item.content.replace("\t", " ").replace("\n", " ")
            fh.write(f"{item.node_id}\t\t{item.pool}\t{content}\n")


def apply_template(path: Path, grades: dict[str, str]) -> tuple[int, int]:
    """Merge template verdicts into grades; returns (applied, skipped).

    Blank verdicts are ungraded rows and are ignored silently; a non-blank
    value outside {correct, incorrect, unsure} counts as skipped.
    """
    applied = skipped = 0
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 2:
                continue
            node_id, verdict = parts[0].strip(), parts[1].strip().lower()
            if not node_id or not verdict:
                continue
            if verdict not in VERDICTS:
                skipped += 1
                continue
            grades[node_id] = verdict
            applied += 1
    return applied, skipped


def grade_interactive(
    items: list[SampleItem],
    grades: dict[str, str],
    infile=None,
    outfile=None,
) -> int:
    """Grade ungraded items; returns count graded this session."""
    infile = infile or sys.stdin
    outfile = outfile or sys.stdout
    done = 0
    for item in items:
        if item.node_id in grades:
            continue
        print(
            f"\n[{item.pool}] {item.concept_type} · status={item.status}"
            f" · node={item.node_id}",
            file=outfile,
        )
        print(item.content, file=outfile)
        print("--- source excerpt ---", file=outfile)
        print(excerpt_for(item.sources), file=outfile)
        print("----------------------", file=outfile)
        while True:
            print(PROMPT, end="", flush=True, file=outfile)
            answer = infile.readline().strip().lower()
            if answer == "q":
                return done
            if answer == "s":
                break
            if answer in KEYMAP:
                grades[item.node_id] = KEYMAP[answer]
                done += 1
                break
            print("answer c, i, u, s or q", file=outfile)
    return done
