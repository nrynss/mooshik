"""Offline suite for the M9 measurement harness.

No network anywhere: the SQL seam is faked with a dict-keyed cursor that
answers by SQL marker. Live verification happens separately against real
Cloud SQL and is recorded in dev-diary/adversarial-review/m9-implementation.md.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import pytest

from measurement import pools
from measurement.excerpt import excerpt_for, resolve_excerpt
from measurement.grade import (
    apply_template,
    grades_path_for,
    load_grades,
    save_grades,
)
from measurement.report import COVERAGE_GATE, render_report
from measurement.sample import draw, read_jsonl, run_sampling, write_jsonl
from measurement.stats import precision, wilson


# --------------------------------------------------------------- fake seam


class FakeConn:
    """Answers queries by a unique SQL marker per pool; rows queued per marker."""

    def __init__(self, results: dict[str, list[dict]]) -> None:
        self.results = results
        self.calls: list[tuple[str, tuple]] = []

    def query(self, sql: str, params: tuple = ()) -> list[dict]:
        self.calls.append((sql, params))
        marker = _marker(sql)
        if marker not in self.results:
            raise AssertionError(f"unexpected SQL: {sql[:60]}...")
        return [dict(r) for r in self.results[marker]]


def _marker(sql: str) -> str:
    # Distinct substrings of the four harness queries.
    if "canonization_status <> 'Canonical'" in sql:
        return "rejected"
    if "AND canonization_status = 'Canonical'" in sql:
        return "canonical"
    if "count(embedding)" in sql:
        return "coverage"
    return "raw"


def concept_row(i: int, *, status="None", source=None, embedded=True):
    return {
        "node_id": f"00000000-0000-0000-0000-{i:012d}",
        "content": f"concept {i}",
        "concept_type": "Entity",
        "status": status,
        "created_at": "2026-08-20T00:00:00+00:00",
        "source_ref": source or (f"document:file:/doc-{i}.md" if source != "" else None),
        "embedded": embedded,
    }


def graph_conn(
    raw_rows: list[dict] | None = None,
    canonical_rows: list[dict] | None = None,
    rejected_rows: list[dict] | None = None,
    coverage: tuple[int, int] = (16, 27),
) -> FakeConn:
    raw_rows = raw_rows if raw_rows is not None else [concept_row(i) for i in range(14)]
    canonical_rows = canonical_rows if canonical_rows is not None else []
    rejected_rows = (
        rejected_rows
        if rejected_rows is not None
        else [concept_row(i, status="Candidate") for i in range(14)]
    )
    return FakeConn({
        "raw": raw_rows,
        "canonical": canonical_rows,
        "rejected": rejected_rows,
        "coverage": [{"embedded": coverage[0], "total": coverage[1]}],
    })



# ---------------------------------------------------------------- sampling


class TestSampling:
    def test_seeded_determinism_same_seed_same_sample(self):
        conn_a = graph_conn()
        a, sizes_a = run_sampling(conn_a, "s", n_raw=10, n_canonical=5, n_rejected=5, seed=7)
        conn_b = graph_conn()
        b, sizes_b = run_sampling(conn_b, "s", n_raw=10, n_canonical=5, n_rejected=5, seed=7)
        assert [x.node_id for x in a] == [x.node_id for x in b]
        assert sizes_a == sizes_b

    def test_different_seed_draws_different_sample(self):
        a, _ = run_sampling(graph_conn(), "s", 10, 0, 0, seed=1)
        b, _ = run_sampling(graph_conn(), "s", 10, 0, 0, seed=2)
        assert [x.node_id for x in a] != [x.node_id for x in b]

    def test_no_duplicates_within_a_pool(self):
        items, _ = run_sampling(graph_conn(), "s", 14, 0, 14, seed=3)
        ids = [x.node_id for x in items if x.pool == "raw"]
        rej_ids = [x.node_id for x in items if x.pool == "rejected"]
        assert len(ids) == len(set(ids))
        assert len(rej_ids) == len(set(rej_ids))

    def test_n_above_pool_size_clamps(self):
        items, sizes = run_sampling(graph_conn(), "s", 99, 99, 99, seed=4)
        assert sizes == {"raw": 14, "canonical": 0, "rejected": 14}
        assert sum(1 for i in items if i.pool == "raw") == 14
        assert sum(1 for i in items if i.pool == "canonical") == 0

    def test_rejected_draw_excludes_already_drawn_nodes(self):
        items, _ = run_sampling(graph_conn(), "s", 10, 0, 10, seed=5)
        drawn = {(x.pool, x.node_id) for x in items}
        per_pool = {}
        for pool, nid in drawn:
            per_pool.setdefault(pool, set()).add(nid)
        overlap = per_pool.get("raw", set()) & per_pool.get("rejected", set())
        assert not overlap, "an item must never be graded twice"

    def test_items_carry_provenance_and_fields(self):
        rows = [concept_row(1, source="document:file:/tmp/x.md")]
        items = draw("raw", rows, 1, seed=0)
        item = items[0]
        assert item.sources == ["document:file:/tmp/x.md"]
        assert item.status and item.created_at and item.concept_type

    def test_jsonl_round_trip(self, tmp_path):
        items, _ = run_sampling(graph_conn(), "s", 6, 2, 3, seed=6)
        path = tmp_path / "sample.jsonl"
        write_jsonl(items, path)
        loaded = read_jsonl(path)
        assert [(i.node_id, i.pool) for i in loaded] == [(i.node_id, i.pool) for i in items]


# -------------------------------------------------------------- statistics


class TestWilson:
    def test_known_value_8_of_10(self):
        lo, hi = wilson(8, 10)
        assert lo == pytest.approx(0.4902, abs=1e-3)
        assert hi == pytest.approx(0.9433, abs=1e-3)

    def test_zero_count_lower_edge(self):
        lo, hi = wilson(0, 10)
        assert lo == 0.0
        assert hi == pytest.approx(0.2775, abs=1e-3)

    def test_all_success_upper_edge(self):
        lo, hi = wilson(10, 10)
        assert hi == pytest.approx(1.0, abs=1e-12)
        assert lo == pytest.approx(0.7225, abs=1e-3)

    def test_empty_population_is_none_not_a_fake_interval(self):
        assert wilson(0, 0) is None

    def test_interval_stays_inside_unit_square_for_extremes(self):
        for k in range(11):
            lo, hi = wilson(k, 10)
            assert 0.0 <= lo <= hi <= 1.0

    def test_precision_excludes_unsure_by_construction(self):
        assert precision(8, 2) == pytest.approx(0.8)
        assert precision(0, 0) is None


# ------------------------------------------------------------------ grading


class TestGrading:
    def test_grades_persist_keyed_by_node_id(self, tmp_path):
        path = tmp_path / "g.grades.jsonl"
        save_grades(path, {"a" * 36: "correct"})
        assert load_grades(path) == {"a" * 36: "correct"}
        save_grades(path, {"b" * 36: "incorrect"})  # merge over, keyed by node id
        merged = load_grades(path)
        assert merged["a" * 36] == "correct"
        assert merged["b" * 36] == "incorrect"

    def test_grades_survive_resampling_merge(self, tmp_path):
        """Re-sampling drops old items; grades file must keep their entries."""
        first, _ = run_sampling(graph_conn(), "s", 14, 0, 0, seed=1)
        path = tmp_path / "g.jsonl"
        save_grades(path, {i.node_id: "correct" for i in first})
        second, _ = run_sampling(graph_conn(), "s", 5, 0, 0, seed=2)
        grades = load_grades(path)
        graded_now = {n for n in (i.node_id for i in second) if n in grades}
        assert len(graded_now) >= 1  # re-sampled items reuse persisted verdicts
        assert len(load_grades(path)) == 14  # nothing dropped

    def test_template_apply_round_trip(self, tmp_path):
        items, _ = run_sampling(graph_conn(), "s", 3, 0, 0, seed=9)
        template = tmp_path / "t.tsv"
        from measurement.grade import write_template

        write_template(items, template)
        lines = template.read_text().splitlines()
        assert len(lines) == 3
        # human edits column 2
        edited = []
        for idx, line in enumerate(lines):
            parts = line.split("\t")
            parts[1] = ["correct", "incorrect", "unsure"][idx]
            edited.append("\t".join(parts))
        template.write_text("\n".join(edited) + "\n")

        grades: dict[str, str] = {}
        applied, skipped = apply_template(template, grades)
        assert applied == 3 and skipped == 0
        assert set(grades.values()) == {"correct", "incorrect", "unsure"}

    def test_apply_ignores_blank_and_bad_verdicts(self, tmp_path):
        template = tmp_path / "t.tsv"
        template.write_text("id-a\tcorrect\tc\nid-b\t\tc\nid-c\tnope\tc\n")
        grades: dict[str, str] = {}
        applied, skipped = apply_template(template, grades)
        assert applied == 1 and skipped == 1
        assert grades == {"id-a": "correct"}

    def test_load_rejects_unknown_verdict(self, tmp_path):
        path = tmp_path / "bad.jsonl"
        path.write_text(json.dumps({"node_id": "x", "verdict": "maybe"}) + "\n")
        with pytest.raises(ValueError):
            load_grades(path)


# ---------------------------------------------------------------- coverage


class TestCoverageAndReport:
    def test_coverage_reads_embedded_vs_total_from_cursor(self):
        conn = graph_conn(coverage=(16, 27))
        assert pools.coverage_overall(conn, "mooshik") == (16, 27)

    def test_report_leads_with_coverage_and_gates_low_values(self):
        items, sizes = run_sampling(graph_conn(), "s", 4, 0, 4, seed=11)
        grades = {i.node_id: "correct" for i in items}
        report = render_report(items, grades, sizes, coverage_embedded=16, coverage_total=27, session="s")
        cov_pos = report.index("Embedding coverage")
        prec_pos = report.index("## Precision")
        assert cov_pos < prec_pos, "coverage must be reported FIRST"
        assert "WARNING" in report
        assert "keyword leg" in report
        assert "59.3%" in report

    def test_report_passes_gate_without_warning(self):
        items, sizes = run_sampling(graph_conn(), "s", 2, 0, 0, seed=12)
        report = render_report(items, {}, sizes, coverage_embedded=100, coverage_total=100, session="s")
        assert "WARNING" not in report
        assert "100.0%" in report

    def test_gate_threshold_is_ninety_percent(self):
        assert COVERAGE_GATE == 0.90

    def test_report_shows_all_three_intervals_and_difference(self):
        items, sizes = run_sampling(graph_conn(), "s", 3, 0, 3, seed=13)
        grades = {i.node_id: ("correct" if i.pool == "raw" else "incorrect") for i in items}
        report = render_report(items, grades, sizes, 30, 30, session="s")
        assert report.count("[0.") >= 1
        assert "raw-extraction" in report
        assert "canonical-fact" in report
        assert "wrongly-rejected" in report
        assert "Difference (raw − canonical)" in report

    def test_empty_canonical_pool_named_as_promoting_nothing(self):
        items, sizes = run_sampling(graph_conn(), "s", 2, 0, 2, seed=14)
        grades = {i.node_id: "correct" for i in items}
        report = render_report(items, grades, sizes, 10, 10, session="s")
        assert "promotes nothing" in report
        assert "n/a" in report  # canonical precision honestly undefined

    def test_per_pool_coverage_table_present(self):
        rows = [
            concept_row(0, source="document:file:/a.md"),
            concept_row(1, source="document:file:/a.md", embedded=False),
        ]
        conn = graph_conn(raw_rows=rows)
        items, sizes = run_sampling(conn, "s", 2, 0, 0, seed=15)
        report = render_report(items, {}, sizes, 50, 60, session="s")
        assert "| raw |" in report


# ----------------------------------------------------------------- excerpts


class TestExcerpts:
    def test_file_ref_reads_head(self, tmp_path):
        doc = tmp_path / "src.md"
        doc.write_text("# Zephyr\nThe fairness quantum is 40ms.\n" + "filler\n" * 200)
        found = resolve_excerpt(f"document:file:{doc}", limit=80)
        assert found is not None and found.startswith("# Zephyr")
        assert len(found) <= 80

    def test_missing_file_returns_none(self):
        assert resolve_excerpt("document:file:/nonexistent/nope.md") is None

    def test_non_document_ref_returns_none(self):
        assert resolve_excerpt("Ingested file x: 4 concepts") is None

    def test_excerpt_for_falls_back_to_refs(self):
        text = excerpt_for(["document:file:/nonexistent.md"])
        assert "not resolvable" in text


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(pytest.main([__file__, "-q"]))
