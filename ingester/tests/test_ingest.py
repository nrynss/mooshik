"""Offline suite: seams only — walker, scanner, chunker, checkpoint,
extraction parsing/retry, writer payloads. No network anywhere."""


import json
import subprocess
from pathlib import Path

import pytest
from ingester import walker
from ingester.chunker import chunk_text
from ingester.checkpoint import Checkpoint
from ingester.config import Settings
from mooshik_common.concepts import parse_concepts
from ingester.extraction import ConceptExtractor
from ingester.pipeline import (
    DocumentReport,
    Report,
    content_hash,
    document_resource,
)
from ingester.secretscan import find_secret


# ---------------------------------------------------------------- walker ----


def test_allowlist_picks_only_listed_extensions(tmp_path):
    (tmp_path / "a.md").write_text("md")
    (tmp_path / "b.txt").write_text("txt")
    (tmp_path / "c.py").write_text("python")
    (tmp_path / "d.rst").write_text("rst")
    (tmp_path / "e.exe").write_text("binary")
    sources = {d.source for d in walker.collect_documents(tmp_path, (".md", ".txt", ".rst"))}
    assert any(s.endswith("a.md") for s in sources)
    assert any(s.endswith("b.txt") for s in sources)
    assert any(s.endswith("d.rst") for s in sources)
    assert not any("c.py" in s or "e.exe" in s for s in sources)


def test_default_extensions_are_the_documented_four():
    assert Settings(root=Path(".")).extensions == (".md", ".markdown", ".txt", ".rst")


@pytest.fixture()
def fixture_repo(tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    env = {
        "GIT_AUTHOR_NAME": "Ingester Test",
        "GIT_AUTHOR_EMAIL": "ingest@example.com",
        "GIT_COMMITTER_NAME": "Ingester Test",
        "GIT_COMMITTER_EMAIL": "ingest@example.com",
        "HOME": str(tmp_path),
    }
    def git(*args):
        subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, env=env)
    git("init", "-q")
    (repo / "secret.md").write_text("# notes\nAKIAABCDEFGHIJKLMNOP = leaked\n")
    git("add", ".")
    git("commit", "-qm", "add secret notes")
    (repo / "README.md").write_text("# readme\n")
    git("add", ".")
    git("commit", "-qm", "add readme\n\nlonger body explaining why")
    return repo


def test_commit_metadata_walker_never_emits_diff_lines(fixture_repo):
    docs = walker.iter_commits(fixture_repo)
    assert len(docs) == 2
    blob = "\n".join(d.text for d in docs)
    for marker in ("diff --git", "+++", "---", "@@", "index ", "leaked"):
        assert marker not in blob, f"patch content marker {marker!r} leaked"
    assert "add readme" in blob
    assert "longer body explaining why" in blob  # subject + body included
    assert all(d.kind == "commit" for d in docs)


def test_files_inside_a_repo_are_not_walked_only_its_metadata(fixture_repo, tmp_path):
    docs = walker.collect_documents(tmp_path, (".md",))
    file_sources = [d for d in docs if d.kind == "file"]
    commit_sources = [d for d in docs if d.kind == "commit"]
    assert len(commit_sources) == 2
    assert not any(d.path and str(d.path).startswith(str(fixture_repo)) for d in file_sources)


def test_skip_dirs_are_never_descended(tmp_path):
    junk = tmp_path / "node_modules" / "x.md"
    junk.parent.mkdir()
    junk.write_text("junk")
    assert walker.collect_documents(tmp_path, (".md",)) == []


def test_symlinks_never_cross_the_corpus_root_boundary(tmp_path):
    root = tmp_path / "corpus"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "elsewhere.md").write_text("content living outside the root\n")
    (root / "inside.md").write_text("inside\n")

    # file symlink inside the root -> target outside the root
    (root / "link.md").symlink_to(outside / "elsewhere.md")
    # directory symlink inside the root -> directory outside the root
    (root / "dir-link").symlink_to(outside, target_is_directory=True)

    sources = [d.source for d in walker.collect_documents(root, (".md",))]
    assert any(s.endswith("inside.md") for s in sources)
    assert not any("link.md" in s for s in sources), sources
    assert not any("elsewhere" in s for s in sources), sources


# --------------------------------------------------------- secretscanner ----


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        ("-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----", "pem-block"),
        ("-----BEGIN OPENSSH PRIVATE KEY-----", "pem-block"),
        ("key AKIAIOSFODNN7EXAMPLE ok", "aws-access-key"),
        ("token ghp_0123456789abcdefghijklmnopqrstuvwxyzABC here", "github-token"),
        ("github_pat_11ABCDEFG0123456789_0123456789012345678901", "github-pat"),
        ("slack xoxb-123456789012-1234567890123-abcdefabcdef", "slack-token"),
        ("API_KEY: c2VjcmV0dmFsdWVzdHJ1Y3R1cmU9PQ==", "generic-assignment"),
        ('PASSWORD="aLongBase64ishLiteralValue1234567890"', "generic-assignment"),
        ("just a normal sentence about tokens of appreciation", None),
        ("AKIA is short", None),
    ],
)
def test_pattern_classes(text, expected):
    assert find_secret(text) == expected


def test_vault_value_from_extra_forbidden_drops_document():
    assert find_secret("my dog's name is ZephyrBreeze-42", ()) is None
    assert (
        find_secret("my dog's name is ZephyrBreeze-42", ("ZephyrBreeze-42",))
        == "vault-value"
    )


def test_empty_extra_values_are_ignored():
    assert find_secret("anything", ("",)) is None


# ---------------------------------------------------------------- chunker ----


def test_short_text_is_one_chunk():
    assert chunk_text("hello") == ["hello"]


def test_chunks_never_exceed_budget_and_have_no_overlap():
    text = "\n\n".join(f"paragraph {i} " + "x" * 80 for i in range(60))
    chunks = chunk_text(text, size=1000)
    assert all(len(c) <= 1000 for c in chunks)
    joined = "\n".join(chunks)
    for i in range(60):
        assert f"paragraph {i}" in joined
    # no overlap: consecutive chunks share no full paragraph
    for a, b in zip(chunks, chunks[1:]):
        overlap = set(a.split("\n")) & set(b.split("\n"))
        assert not overlap


def test_oversized_line_is_hard_sliced():
    line = "y" * 2501
    chunks = chunk_text(line, size=1000)
    assert [len(c) for c in chunks] == [1000, 1000, 501]
    assert "".join(chunks) == line


def test_empty_and_whitespace_documents_yield_nothing():
    assert chunk_text("") == []
    assert chunk_text("   \n  \n ") == []


# ------------------------------------------------------------ checkpoint ----


def test_checkpoint_roundtrip_and_resume_skip(tmp_path):
    state = tmp_path / "state.json"
    cp = Checkpoint(state)
    key_a = Checkpoint.key("file:a.md", content_hash("A"))
    key_b = Checkpoint.key("file:b.md", content_hash("B"))
    assert cp.status(key_a) is None
    cp.mark(key_a)
    cp.mark(key_b, "dropped-secret")

    again = Checkpoint(state)
    assert again.status(key_a) == "done"
    assert again.status(key_b) == "dropped-secret"


def test_changed_content_gets_a_new_key():
    assert Checkpoint.key("f", content_hash("v1")) != Checkpoint.key("f", content_hash("v2"))


def test_corrupt_state_starts_clean_instead_of_crashing(tmp_path):
    state = tmp_path / "state.json"
    state.write_text("{not json")
    assert Checkpoint(state).status("k") is None


# ------------------------------------------------------------ extraction ----


class FakeResponse:
    def __init__(self, text):
        self.text = text


class FakeClient:
    def __init__(self, responses):
        self.responses = list(responses)
        self.prompts = []

    def generate_content(self, model, contents):
        self.prompts.append(contents)
        item = self.responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return FakeResponse(item)


VALID = json.dumps(
    [
        {"content": "Mooshik uses Cloud SQL Postgres", "concept_type": "constraint"},
        {"content": "", "concept_type": "entity"},  # empty → dropped
        {"content": "weird", "concept_type": "emotion"},  # bad type → dropped
        "not-a-dict",  # dropped
    ]
)


def test_parse_keeps_only_valid_typed_concepts():
    concepts = parse_concepts(VALID)
    assert [(c.content, c.concept_type) for c in concepts] == [
        ("Mooshik uses Cloud SQL Postgres", "constraint")
    ]


def test_parse_strips_code_fences_and_surrounding_prose():
    raw = 'Here you go:\n```json\n[{"content":"x","concept_type":"logic"}]\n```\nDone.'
    assert [c.content for c in parse_concepts(raw)] == ["x"]


def _extractor(responses, clock=lambda s: None, **kw):
    return ConceptExtractor(FakeClient(responses), clock=clock, sleep_secs=0, **kw)

class FakeModels:
    def __init__(self, client):
        self._client = client

    def generate_content(self, model, contents):
        client = self._client
        client.prompts.append(contents)
        item = client.responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return FakeResponse(item)


class FakeClient:
    def __init__(self, responses):
        self.responses = list(responses)
        self.prompts = []
        self.models = FakeModels(self)


def test_unparseable_chunk_retried_once_then_skipped():
    ext = _extractor(["utter garbage", "still garbage"])
    assert ext.extract("chunk") == []
    assert ext.skipped_chunks == 1

def test_rate_limit_backs_off_then_succeeds():
    waits = []
    err = RuntimeError("429 RESOURCE_EXHAUSTED: quota exceeded")
    ext = _extractor([err, err, '[{"content":"ok","concept_type":"entity"}]'],
                     clock=waits.append)
    assert [c.content for c in ext.extract("chunk")] == ["ok"]
    assert [w for w in waits if w] == [1.0, 2.0]  # exponential backoff



def test_non_rate_limit_errors_raise_immediately():
    ext = _extractor([RuntimeError("500 boom")])
    with pytest.raises(RuntimeError):
        ext.extract("chunk")


# --------------------------------------------------------------- pipeline ----


class FakeWriter:
    """The MCP seam, faked: records every payload for assertion."""

    def __init__(self):
        self.derives = []
        self.actions = []
        self.derive_event_times = []
        self.action_event_times = []
        self.stats_responses = [{"log_depth": 0}]
        self.stats_calls = 0

    async def _call(self, tool: str, arguments: dict):
        if tool != "lambo_stats":
            raise AssertionError(f"unexpected tool call: {tool}")
        response = self.stats_responses[
            min(self.stats_calls, len(self.stats_responses) - 1)
        ]
        self.stats_calls += 1
        return response

    async def derive(self, agent_id, concepts, parent_of=None, event_time=None):
        self.derives.append((agent_id, concepts, parent_of))
        self.derive_event_times.append(event_time)

    async def record_action(
        self,
        agent_id,
        action,
        produces=None,
        modifies=None,
        depends_on=None,
        event_time=None,
    ):
        self.actions.append((agent_id, action, produces))
        self.action_event_times.append(event_time)


def _make_settings(tmp_path, **kw):
    return Settings(root=tmp_path, state_path=tmp_path / "state.json", **kw)


def _run(settings, docs_root):
    import asyncio

    from ingester.pipeline import ingest

    writer = FakeWriter()
    extractor = _extractor(
        ['[{"content":"alpha concept","concept_type":"entity"},'
         '{"content":"beta rule","concept_type":"constraint"}]']
        * 50
    )
    report = asyncio.run(ingest(settings, writer, extractor))
    return writer, extractor, report


def test_derive_payloads_carry_provenance_and_valid_types(tmp_path):
    (tmp_path / "note.md").write_text("# Note\nsome durable fact about the workspace.\n" * 5)
    settings = _make_settings(tmp_path)
    writer, _, report = run = _run(settings, tmp_path)

    agent_id, concepts, parent_of = writer.derives[0]
    assert agent_id == "bootstrap"
    source = f"file:{tmp_path / 'note.md'}"
    assert all(c["concept_type"] in {"entity", "logic", "constraint", "resource", "observation"}
               for c in concepts)
    assert parent_of == [{"parent": document_resource(source), "child": c["content"]}
                         for c in concepts]
    _, action, produces = writer.actions[0]
    assert action.startswith("Ingested file ")
    assert produces == [document_resource(source)]
    assert report.written == 1


def test_secret_hit_drops_whole_document_before_any_write(tmp_path):
    (tmp_path / "clean.md").write_text("clean note\n")
    (tmp_path / "dirty.md").write_text(
        "# dirty\n-----BEGIN RSA PRIVATE KEY-----\nabc\n"
    )
    settings = _make_settings(tmp_path)
    writer, extractor, report = _run(settings, tmp_path)

    assert [s for s, _ in report.dropped] == [f"file:{tmp_path / 'dirty.md'}"]
    assert report.dropped[0][1] == "pem-block"
    written_sources = [d.source for d in report.documents if d.status == "written"]
    assert written_sources == [f"file:{tmp_path / 'clean.md'}"]


def test_second_run_resumes_and_never_rewrites(tmp_path):
    (tmp_path / "note.md").write_text("durable note\n")
    settings = _make_settings(tmp_path)
    first_writer, _, first = _run(settings, tmp_path)
    assert first.written == 1

    second_writer, second_extractor, second = _run(settings, tmp_path)
    assert second.written == 0
    assert second.resumed == 1
    assert second_writer.derives == []          # never re-derived
    assert second_extractor.calls == 0          # never re-called Vertex


def test_dry_run_scan_only_makes_no_calls(tmp_path):
    from ingester.pipeline import plan

    (tmp_path / "n.md").write_text("note\n")
    kept, report = plan(_make_settings(tmp_path))
    assert len(kept) == 1
    assert report.candidates == 1


def test_report_defaults_shape():
    report = Report()
    assert report.documents == [] and isinstance(report.documents, list)
    assert DocumentReport("s", "written").concepts == 0


# ------------------------------------------------------------ event time ----
#
# Lambo's solo promotion policy counts recurrence over event time, never flush
# stamps. Before these pins a decade of history landed as one afternoon and
# canonization promoted nothing — the pathology M9 measured.


@pytest.fixture()
def dated_repo(tmp_path):
    """Two commits authored a year apart, so recurrence has real spread."""
    repo = tmp_path / "dated"
    repo.mkdir()
    base = {
        "GIT_AUTHOR_NAME": "Ingester Test",
        "GIT_AUTHOR_EMAIL": "ingest@example.com",
        "GIT_COMMITTER_NAME": "Ingester Test",
        "GIT_COMMITTER_EMAIL": "ingest@example.com",
        "HOME": str(tmp_path),
    }

    def git(*args, when=None):
        env = dict(base)
        if when:
            # Author date is the historical claim; committer date is not.
            env["GIT_AUTHOR_DATE"] = when
            env["GIT_COMMITTER_DATE"] = when
        subprocess.run(
            ["git", "-C", str(repo), *args], check=True, capture_output=True, env=env
        )

    git("init", "-q")
    (repo / "a.md").write_text("first\n")
    git("add", ".")
    git("commit", "-qm", "first commit", when="2021-03-04T05:06:07+00:00")
    (repo / "b.md").write_text("second\n")
    git("add", ".")
    git("commit", "-qm", "second commit", when="2022-03-04T05:06:07+00:00")
    return repo


def test_commit_documents_carry_the_author_date_as_event_time(dated_repo):
    docs = walker.iter_commits(dated_repo)
    stamps = sorted(d.event_time for d in docs)
    assert stamps == [
        "2021-03-04T05:06:07+00:00",
        "2022-03-04T05:06:07+00:00",
    ], f"author dates must survive as RFC3339 UTC, got {stamps}"


def test_an_unparseable_commit_date_degrades_to_a_live_fact():
    # Never fail the document over its date: the extraction is still worth
    # having, only its recurrence evidence is lost.
    assert walker._parse_git_date("not-a-date") is None
    assert walker._parse_git_date("") is None
    assert walker._parse_git_date("2021-03-04T05:06:07Z") == "2021-03-04T05:06:07+00:00"


def test_file_documents_carry_mtime_as_event_time(tmp_path):
    import os
    from datetime import datetime, timezone

    note = tmp_path / "note.md"
    note.write_text("a durable fact\n")
    when = datetime(2019, 7, 1, 12, 0, 0, tzinfo=timezone.utc)
    os.utime(note, (when.timestamp(), when.timestamp()))

    docs = walker.collect_documents(tmp_path, (".md",))
    assert [d.event_time for d in docs] == ["2019-07-01T12:00:00+00:00"]


def test_every_write_carries_the_documents_event_time(tmp_path):
    """The load-bearing pin: a write that drops event_time silently reverts
    the graph to flush-time recurrence, and nothing ever promotes again."""
    import os
    from datetime import datetime, timezone

    note = tmp_path / "note.md"
    note.write_text("# Note\nsome durable fact about the workspace.\n" * 5)
    when = datetime(2018, 1, 2, 3, 4, 5, tzinfo=timezone.utc)
    os.utime(note, (when.timestamp(), when.timestamp()))

    settings = _make_settings(tmp_path)
    writer, _, report = _run(settings, tmp_path)

    assert report.written == 1
    expected = "2018-01-02T03:04:05+00:00"
    assert writer.derive_event_times == [expected]
    assert writer.action_event_times == [expected]


def test_writer_sends_event_time_only_when_the_document_has_one():
    """Absent event_time must not reach the wire as an explicit null — the
    field is optional and omitting it means 'a live fact, about now'."""
    import asyncio

    from ingester.writer import LamboMcpWriter

    sent = []

    writer = LamboMcpWriter.__new__(LamboMcpWriter)

    async def fake_call(tool, arguments):
        sent.append((tool, arguments))
        return None

    writer._call = fake_call

    asyncio.run(writer.derive("a", [{"content": "c", "concept_type": "entity"}]))
    asyncio.run(
        writer.derive(
            "a",
            [{"content": "c", "concept_type": "entity"}],
            event_time="2020-05-06T07:08:09+00:00",
        )
    )
    asyncio.run(writer.record_action("a", "did a thing"))
    asyncio.run(
        writer.record_action("a", "did a thing", event_time="2020-05-06T07:08:09+00:00")
    )

    assert "event_time" not in sent[0][1]
    assert sent[1][1]["event_time"] == "2020-05-06T07:08:09+00:00"
    assert "event_time" not in sent[2][1]
    assert sent[3][1]["event_time"] == "2020-05-06T07:08:09+00:00"


#: The workspace corpus is built so canonization has something to earn: each
#: fact recurs on a planned set of days, and `solo_score = sessions x
#: eviction_resistance` puts it in a chosen band (bars 3 / 6 / 10). Editing a
#: fixture and breaking one of these sentences would silently flatten the
#: ladder — the graph would look fine and promote less, which is exactly the
#: failure this corpus exists to detect.
LADDER = [
    (
        "The Windpipe ring never holds more than 512 in-flight messages;"
        " overflow writers block instead of dropping.",
        [21, 22, 23, 24, 25, 26, 27],
        1.5,
        "Canonical",
    ),
    (
        "Secrets never enter the graph: the vault is the only place a"
        " credential value lives.",
        [21, 22, 24, 26, 27],
        1.5,
        "Venerable",
    ),
    (
        "The Quillstone build cache lives on the shared NAS under /srv/quillstone.",
        [21, 23, 24, 25, 27],
        1.2,
        "Venerable",
    ),
    (
        "The Zephyr scheduler assigns every task a fairness quantum of exactly"
        " 40 milliseconds.",
        [22, 23, 26],
        1.2,
        "Candidate",
    ),
    (
        "Cobalt Lantern retries failed fetches three times with jitter.",
        [23, 25],
        1.5,
        "Candidate",
    ),
]


def _band(score: float) -> str:
    if score >= 10.0:
        return "Canonical"
    if score >= 6.0:
        return "Venerable"
    if score >= 3.0:
        return "Candidate"
    return "None"


def _corpus_files():
    root = Path(__file__).resolve().parents[2] / "ingest-fixtures" / "workspace"
    return sorted(root.glob("*.md"))


def test_workspace_corpus_recurrence_earns_the_planned_canonization_ladder():
    files = _corpus_files()
    assert len(files) >= 40, f"corpus looks truncated: {len(files)} documents"

    texts = {path.name: path.read_text() for path in files}
    for sentence, want_days, resistance, want_band in LADDER:
        days = sorted({int(name[8:10]) for name, body in texts.items() if sentence in body})
        assert days == sorted(want_days), (
            f"recurrence changed for {sentence[:40]!r}: on days {days}, "
            f"planned {sorted(want_days)}. A line-wrapped or reworded copy "
            f"does not count — the sentence must appear whole."
        )
        score = len(days) * resistance
        assert _band(score) == want_band, (
            f"{sentence[:40]!r} scores {score} -> {_band(score)}, planned {want_band}"
        )


def test_every_workspace_document_is_dated_in_its_filename():
    """`backdate.py` replays these dates onto mtimes, which is the only reason
    the corpus has any event-time spread. An undated file silently ingests as
    a live fact."""
    import re

    undated = [p.name for p in _corpus_files() if not re.match(r"^\d{4}-\d{2}-\d{2}-", p.name)]
    assert not undated, f"undated corpus documents: {undated}"

    days = {p.name[:10] for p in _corpus_files()}
    assert len(days) == 7, f"expected a 7-day spread, got {sorted(days)}"


def test_workspace_corpus_carries_no_secret_that_would_drop_a_document():
    """The scanner drops a whole document on a hit. A fixture that trips it
    would remove its day's recurrence without any other signal."""
    from ingester.secretscan import find_secret

    hits = [(p.name, find_secret(p.read_text(), ())) for p in _corpus_files()]
    dropped = [(name, hit) for name, hit in hits if hit]
    assert not dropped, f"documents would be dropped: {dropped}"


def test_the_image_ships_one_binary_so_the_serve_child_cannot_drift():
    """The writer speaks MCP to a `serve` child. That child used to be a
    separately-installed `lambo` pinned by SHA — a second copy of the same
    code, free to drift from the library Mooshik links. It did drift: the
    image sat on a rev whose write params predated `event_time`, and since
    they are deny_unknown_fields it would have rejected every historical
    write rather than ignoring the field.

    Building `mooshik` from this checkout makes the skew impossible instead
    of merely tested for, so this asserts the property rather than comparing
    two revisions that no longer both exist.
    """
    root = Path(__file__).resolve().parents[2]
    dockerfile = (root / "ingester" / "Dockerfile").read_text()

    assert "cargo install" not in dockerfile, (
        "the image must not install a second copy of lambo; build mooshik "
        "from this checkout so the serve child is the code we test"
    )
    assert "cargo build --release --locked" in dockerfile, (
        "--locked, or the image can resolve a different lambo than the "
        "lockfile this repo tests against"
    )
    assert "/usr/local/bin/mooshik" in dockerfile

    entrypoint = (root / "ingester" / "deploy" / "entrypoint.sh").read_text()
    assert "mooshik serve" in entrypoint, "the serve child must be mooshik"
    assert "mooshik init" in entrypoint, (
        "`mooshik serve` opens an existing home, so init must run first"
    )


# ---------------------------------------------------------------- writer ----


def test_the_promotion_policy_reaches_the_serve_child(monkeypatch):
    """Without this the child canonizes under lambo's default, Swarm.

    Swarm promotes on independent agents converging, which a single-writer
    bootstrap never does — so the graph would fill up and promote nothing,
    exactly the pathology M9 measured. The child env is a strict allowlist,
    so a variable that is not named there is stripped and the failure is
    silent: a healthy-looking ingest with an empty canonical pool.
    """
    from ingester.writer import LamboMcpWriter

    monkeypatch.setenv("LAMBO_PROMOTION_POLICY", "Solo")
    env = LamboMcpWriter._build_params(["lambo", "serve"]).env
    assert env is not None
    assert env.get("LAMBO_PROMOTION_POLICY") == "Solo", (
        "the serve child must inherit the promotion policy, or it runs Swarm "
        "and a single-writer graph can never promote"
    )


def test_writer_child_env_is_an_allowlist_not_wholesale_inheritance(monkeypatch):
    from ingester.writer import LamboMcpWriter

    # planted canaries: secrets from the parent env must not reach the child
    monkeypatch.setenv("MOOSHIK_VAULT_PASSPHRASE", "CANARY-vault-passphrase")
    monkeypatch.setenv("AWS_SESSION_TOKEN", "CANARY-aws-token")
    monkeypatch.setenv("SOME_RANDOM_FAMILY_SECRET", "CANARY-other")
    # documented store/embedder config the serve child does need
    monkeypatch.setenv("LAMBO_STORE", "postgres")
    monkeypatch.setenv("LAMBO_EMBEDDER", "gemini")
    monkeypatch.setenv("LAMBO_POSTGRES_DSN", "postgres://user@host/db")
    monkeypatch.setenv("PATH", "/usr/bin:/bin")

    params = LamboMcpWriter._build_params(["lambo", "serve"])
    env = params.env
    assert env is not None
    assert "MOOSHIK_VAULT_PASSPHRASE" not in env
    assert "AWS_SESSION_TOKEN" not in env
    assert "SOME_RANDOM_FAMILY_SECRET" not in env
    assert env["LAMBO_STORE"] == "postgres"
    assert env["LAMBO_EMBEDDER"] == "gemini"
    assert env["LAMBO_POSTGRES_DSN"] == "postgres://user@host/db"
    assert set(env) <= set(LamboMcpWriter._CHILD_ENV_ALLOWLIST)


def test_pipeline_drains_write_behind_log_after_writes(tmp_path):
    """The pipeline must hold the child open until the write-behind log
    drains — an abrupt exit discards the un-embedded tail (J3)."""
    import asyncio
    from ingester.pipeline import ingest

    (tmp_path / "note.md").write_text("# Note\na durable fact for the drain pin.\n" * 5)
    settings = _make_settings(tmp_path)
    writer = FakeWriter()
    extractor = _extractor(
        ['[{"content":"drain pin concept","concept_type":"entity"}]'] * 50
    )
    report = asyncio.run(ingest(settings, writer, extractor))

    assert report.written == 1
    assert writer.stats_calls >= 1


def test_drain_polls_until_log_depth_reaches_zero():
    import asyncio
    from ingester.writer import drain

    class SeqWriter:
        def __init__(self, responses):
            self.responses = list(responses)
            self.calls = 0

        async def _call(self, tool, arguments):
            assert tool == "lambo_stats"
            response = self.responses[min(self.calls, len(self.responses) - 1)]
            self.calls += 1
            return response

    async def scenario():
        drained = await drain(SeqWriter([{"log_depth": 2}, {"log_depth": 0}]),
                              "bootstrap", timeout=5.0, poll=0.01)
        assert drained is True
        stalled = SeqWriter([{"log_depth": 2}])
        gave_up = await drain(stalled, "bootstrap", timeout=0.1, poll=0.01)
        assert gave_up is False

    asyncio.run(scenario())


def test_a_failed_tool_call_is_detected_on_the_real_result_model():
    """The regression this closes: a failed MCP call read as a success.

    `mcp` names the flag `is_error` and treats `isError` as the wire alias
    only, so reading the camelCase name off the model raises AttributeError
    and a getattr default swallowed it. Every failed `lambo_derive` then
    parsed its own error text as the payload and the document was
    checkpointed done — a silent write loss on the failure path.

    Built from the real `mcp` model, not a stand-in, because a hand-rolled
    fake with an `isError` attribute is exactly what hid this.
    """
    from mcp.types import CallToolResult, TextContent

    from ingester.writer import tool_failed

    failed = CallToolResult(
        content=[TextContent(type="text", text="boom: it failed")], isError=True
    )
    ok = CallToolResult(content=[TextContent(type="text", text="{}")])

    assert tool_failed(failed) is True, "a real error result must be detected"
    assert tool_failed(ok) is False
    # Objects carrying neither spelling must not read as failures.
    assert tool_failed(object()) is False


def test_drain_reads_the_rendered_report_the_server_actually_sends():
    """The regression this closes: `lambo_stats` answers in human-readable
    text, not JSON, so a dict-only reading never matched. The gate then
    polled to timeout and warned about data loss on every healthy run —
    inert exactly where it was supposed to protect (Cloud Run Jobs).

    The strings below are verbatim from a real `lambo serve` (sqlite +
    fixture embedder) driven over stdio by the production writer.
    """
    import asyncio

    from ingester.writer import drain, log_depth

    busy = (
        "session 'wire-check' (owner agent 'lambo-serve')\n"
        "nodes=10 edges=11 concepts=5 canonical=0\nembedded=3/5\n"
        "flush_lag=3.107458ms log_depth=6 flush_depth=0 dead_lettered=0 degraded=false\n"
        "epoch=4 daemon_cycles=2 canonization_cycles=0 canonization_failures=0"
    )
    idle = busy.replace("log_depth=6", "log_depth=0")

    assert log_depth(busy) == 6
    assert log_depth(idle) == 0
    # flush_depth=0 must not be mistaken for log_depth.
    assert log_depth("flush_depth=0 log_depth=7") == 7
    # Forward insurance if the tool ever answers JSON, and safe on junk.
    assert log_depth({"log_depth": 0}) == 0
    assert log_depth("no depth here") is None
    assert log_depth(None) is None

    class SeqWriter:
        def __init__(self, responses):
            self.responses = list(responses)
            self.calls = 0

        async def _call(self, tool, arguments):
            response = self.responses[min(self.calls, len(self.responses) - 1)]
            self.calls += 1
            return response

    async def scenario():
        assert await drain(SeqWriter([busy, idle]), "a", timeout=5.0, poll=0.01) is True
        assert await drain(SeqWriter([busy]), "a", timeout=0.1, poll=0.01) is False

    asyncio.run(scenario())


def test_drain_waits_for_the_flush_queue_not_only_the_log():
    """The regression this closes: a false green on the durability gate.

    lambo documents `log_depth.max(flush_depth)` as "the honest lower bound"
    of the loss window — log_depth is the graph's write-behind log,
    flush_depth is the flush task's pending batch. Gating on the log alone
    passed while lambo warned in the same run that 34 acked writes had not
    drained, and left 55 write intents pending: the ingest reported durable
    writes over an incomplete graph.

    Strings are the real rendered report shape.
    """
    import asyncio

    from ingester.writer import drain, undrained

    def report(log: int, flush: int) -> str:
        return (
            "session 'x' (owner agent 'a')\nnodes=1 edges=1 concepts=1 canonical=0\n"
            f"embedded=1/1\nflush_lag=1ms log_depth={log} flush_depth={flush} "
            "dead_lettered=0 degraded=false\nepoch=1 daemon_cycles=1 "
            "canonization_cycles=1 canonization_failures=0"
        )

    # The exact shape that fooled the old gate: log empty, flush is not.
    assert undrained(report(0, 34)) == 34
    assert undrained(report(6, 0)) == 6
    assert undrained(report(0, 0)) == 0
    assert undrained({"log_depth": 0, "flush_depth": 7}) == 7
    assert undrained("no depths here") is None

    class SeqWriter:
        def __init__(self, responses):
            self.responses = list(responses)
            self.calls = 0

        async def _call(self, tool, arguments):
            r = self.responses[min(self.calls, len(self.responses) - 1)]
            self.calls += 1
            return r

    async def scenario():
        # Must NOT return true while the flush queue is still holding writes.
        stalled = SeqWriter([report(0, 34)])
        assert await drain(stalled, "a", timeout=0.1, poll=0.01) is False
        # Returns true only when both are clear.
        ok = SeqWriter([report(0, 34), report(0, 0)])
        assert await drain(ok, "a", timeout=5.0, poll=0.01) is True

    asyncio.run(scenario())


def test_drain_survives_stats_errors_and_still_times_out():
    import asyncio
    from ingester.writer import drain

    class FlakyWriter:
        def __init__(self):
            self.calls = 0

        async def _call(self, tool, arguments):
            self.calls += 1
            raise RuntimeError("child busy")

    async def scenario():
        writer = FlakyWriter()
        gave_up = await drain(writer, "bootstrap", timeout=0.1, poll=0.01)
        assert gave_up is False
        assert writer.calls >= 1

    asyncio.run(scenario())


# ------------------------------------------------------------------- adk ----
# The milestone's ADK shape. `pipeline.py` drives `google-genai` directly (see
# agent.py's header for why a Runner is the wrong fit for a deterministic map
# over chunks), so nothing in the run path constructs the LlmAgent. These are
# the tests that do — without them the ADK surface is unreachable code that no
# run and no assertion ever touches, and the claim in agent.py's docstring that
# it carries "the same instruction, the same model" is unchecked.


@pytest.fixture
def adk_writer():
    """A FakeWriter installed in the agent's module-global writer slot.

    `record_concepts` is a plain callable because ADK function tools must be,
    so the writer reaches it through a module global rather than an argument.
    Restore it or the fake leaks into every later test in the session.
    """
    from ingester import agent as agent_mod

    previous = agent_mod._writer
    writer = FakeWriter()
    agent_mod.use_writer(writer)
    try:
        yield writer
    finally:
        agent_mod._writer = previous


def test_the_adk_agent_carries_the_same_model_and_instruction_as_the_batch_path():
    from ingester.agent import INGEST_INSTRUCTION, build_agent
    from ingester.config import DEFAULT_MODEL
    from ingester.extraction import PROMPT

    agent = build_agent()

    assert agent.name == "bootstrap_ingester"
    # One constant, not a second literal that can drift from the batch path.
    assert agent.model == DEFAULT_MODEL
    assert agent.instruction == INGEST_INSTRUCTION
    assert PROMPT in agent.instruction


def test_the_adk_agent_takes_a_model_override():
    from ingester.agent import build_agent

    assert build_agent(model="gemini-3.5-flash").model == "gemini-3.5-flash"


def test_adk_accepts_the_writer_bridge_as_a_function_tool():
    """ADK wraps the plain callable, which is the half that can actually fail:
    a tool whose signature or annotations it rejects raises at construction."""
    import asyncio

    from ingester.agent import build_agent

    tools = asyncio.run(build_agent().canonical_tools())

    assert [t.name for t in tools] == ["record_concepts"]
    assert type(tools[0]).__name__ == "FunctionTool"


def test_the_function_tool_writes_through_the_same_seam_as_the_pipeline(adk_writer):
    from ingester.agent import record_concepts

    source = "file:/tmp/note.md"
    result = json.loads(
        record_concepts(
            json.dumps(
                [
                    {"source": source, "content": "alpha concept",
                     "concept_type": "entity"},
                    {"source": source, "content": "beta rule",
                     "concept_type": "constraint"},
                ]
            )
        )
    )

    assert "error" not in result
    agent_id, concepts, parent_of = adk_writer.derives[0]
    assert agent_id == "bootstrap"
    assert concepts == [
        {"content": "alpha concept", "concept_type": "entity"},
        {"content": "beta rule", "concept_type": "constraint"},
    ]
    # Same provenance shape the batch path asserts above.
    assert parent_of == [
        {"parent": source, "child": "alpha concept"},
        {"parent": source, "child": "beta rule"},
    ]


def test_the_function_tool_reports_a_missing_writer_instead_of_raising():
    """ADK surfaces a tool's return value to the model; an exception escaping
    a function tool aborts the turn instead of telling it what went wrong."""
    from ingester import agent as agent_mod

    previous = agent_mod._writer
    agent_mod._writer = None
    try:
        result = json.loads(agent_mod.record_concepts("[]"))
    finally:
        agent_mod._writer = previous

    assert result == {"error": "no writer configured"}
