"""Markdown report: coverage first, then precision with Wilson intervals.

Section order is the interpretation order (M9 brief): **embedding coverage
first**, gating everything recall-adjacent — below the 90% gate the keyword
leg dominates recall and any precision number measures the wrong system.
Then raw-extraction precision, canonical-fact precision, their difference,
and the wrongly-rejected rate — "a filter that promotes nothing has perfect
precision and no value", so an empty canonical pool is reported as exactly
that, not skipped.
"""

from __future__ import annotations

from dataclasses import dataclass

from .sample import SampleItem
from .stats import fmt_pct, precision, wilson

COVERAGE_GATE = 0.90


@dataclass
class PoolTally:
    pool: str
    total: int
    correct: int
    incorrect: int
    unsure: int
    ungraded: int

    @property
    def prec(self) -> float | None:
        return precision(self.correct, self.incorrect)

    @property
    def interval(self) -> tuple[float, float] | None:
        return wilson(self.correct, self.correct + self.incorrect)


def tally(pool: str, items: list[SampleItem], grades: dict[str, str]) -> PoolTally:
    t = PoolTally(pool=pool, total=len(items), correct=0, incorrect=0, unsure=0, ungraded=0)
    for item in items:
        verdict = grades.get(item.node_id)
        if verdict == "correct":
            t.correct += 1
        elif verdict == "incorrect":
            t.incorrect += 1
        elif verdict == "unsure":
            t.unsure += 1
        else:
            t.ungraded += 1
    return t


def _interval_cell(t: PoolTally) -> str:
    iv = t.interval
    if iv is None:
        return "n/a"
    lo, hi = iv
    return f"[{lo:.3f}, {hi:.3f}]"


def render_report(
    items: list[SampleItem],
    grades: dict[str, str],
    pool_sizes: dict[str, int],
    coverage_embedded: int,
    coverage_total: int,
    session: str,
) -> str:
    by_pool = {p: [i for i in items if i.pool == p] for p in ("raw", "canonical", "rejected")}
    tallies = {p: tally(p, by_pool[p], grades) for p in by_pool}

    cov_ratio = (coverage_embedded / coverage_total) if coverage_total else 0.0
    lines: list[str] = []
    add = lines.append

    add("# M9 measurement report")
    add("")
    add(f"Session `{session}` · sampled {len(items)} items "
        f"(raw {tallies['raw'].total}/{pool_sizes.get('raw', 0)}, "
        f"canonical {tallies['canonical'].total}/{pool_sizes.get('canonical', 0)}, "
        f"rejected {tallies['rejected'].total}/{pool_sizes.get('rejected', 0)}) · "
        f"{len(grades)} grade(s) on file")
    add("")

    # --- embedding coverage FIRST, gating interpretation -------------------
    add("## Embedding coverage (read this before any recall claim)")
    add("")
    add(f"Concepts with a durable embedding vector: "
        f"**{coverage_embedded}/{coverage_total} ({fmt_pct(cov_ratio)})**.")
    if coverage_total == 0:
        add("Empty graph — nothing to interpret.")
    elif cov_ratio < COVERAGE_GATE:
        add("")
        add(f"> **WARNING: coverage below the {int(COVERAGE_GATE * 100)}% gate.** "
            f"{coverage_total - coverage_embedded} concept(s) have no durable "
            "embedding; recall over them runs on the keyword leg only. Any "
            "recall/precision number over this corpus measures the wrong "
            "system — treat every interval below as descriptive of extraction "
            "fidelity alone.")
    per_pool_rows = [
        p for p in by_pool
        if by_pool[p]
    ]
    if per_pool_rows:
        add("")
        add("| pool | embedded / total |")
        add("|---|---|")
        for p in per_pool_rows:
            emb = sum(1 for i in by_pool[p] if i.embedded)
            add(f"| {p} | {emb} / {len(by_pool[p])} |")
    add("")

    # --- precision ---------------------------------------------------------
    add("## Precision")
    add("")
    add("Verdicts against source documents; `unsure` excluded from both "
        "numerator and denominator; Wilson score interval, 95%.")
    add("")
    add("| population | n graded | correct | precision | Wilson 95% |")
    add("|---|---|---|---|---|")
    for p, label in (
        ("raw", "raw-extraction"),
        ("canonical", "canonical-fact"),
        ("rejected", "wrongly-rejected rate"),
    ):
        t = tallies[p]
        add(f"| {label} ({p}) | {t.correct + t.incorrect} | {t.correct} | "
            f"{fmt_pct(t.prec)} | {_interval_cell(t)} |")
    add("")

    diff = None
    if tallies["raw"].prec is not None and tallies["canonical"].prec is not None:
        diff = tallies["raw"].prec - tallies["canonical"].prec
    add(f"Difference (raw − canonical): **{fmt_pct(diff)}**"
        + ("" if diff is None else f" (raw {fmt_pct(tallies['raw'].prec)}, "
           f"canonical {fmt_pct(tallies['canonical'].prec)})"))
    add("")

    # --- canonization rejections ------------------------------------------
    rej = tallies["rejected"]
    add("## What canonization wrongly rejected")
    add("")
    add(f"Non-promoted extracted concepts graded true per source: "
        f"{rej.correct} of {rej.correct + rej.incorrect} graded "
        f"({fmt_pct(rej.prec)}), Wilson 95% {_interval_cell(rej)}. "
        f"{rej.unsure} unsure, {rej.ungraded} ungraded.")
    if pool_sizes.get("canonical", 0) == 0:
        add("")
        add("> The canonical pool is empty: this filter promotes nothing, so "
            "its precision is trivially perfect and carries no information. "
            "The wrong-rejection rate above is the only live signal about "
            "canonization quality on this corpus.")
    if rej.ungraded or tallies['raw'].ungraded:
        add("")
        add(f"*Ungraded items remain: raw {tallies['raw'].ungraded}, "
            f"rejected {rej.ungraded}. Intervals cover graded items only.*")
    add("")
    return "\n".join(lines)
