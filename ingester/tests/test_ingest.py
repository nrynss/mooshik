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
from ingester.extraction import ConceptExtractor, parse_concepts
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


def test_dockerfile_lambo_rev_matches_the_workspace_pin():
    """The image's `lambo serve` child must be the rev Mooshik pins.

    Its wire params are deny_unknown_fields, so a child older than the
    event_time rev rejects every historical write — the whole ingest fails,
    not just its dates. Drift here is invisible until a live run.
    """
    import re

    root = Path(__file__).resolve().parents[2]
    cargo = (root / "Cargo.toml").read_text()
    dockerfile = (root / "ingester" / "Dockerfile").read_text()

    pinned = re.search(r'lambo = \{ git = "[^"]+", rev = "([0-9a-f]{40})"', cargo)
    baked = re.search(r"--rev ([0-9a-f]{40})", dockerfile)
    assert pinned and baked, "both pins must be readable"
    assert baked.group(1) == pinned.group(1), (
        f"Dockerfile bakes lambo {baked.group(1)[:7]} but Cargo.toml pins "
        f"{pinned.group(1)[:7]} — the serve child will reject writes"
    )


# ---------------------------------------------------------------- writer ----


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
