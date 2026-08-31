from __future__ import annotations
from typing import Any

class FakeResponse:
    def __init__(self, text: str | None):
        self._text = text
        self.usage_metadata = None
        class Part:
            def __init__(self, t):
                self.text = t
        class Content:
            def __init__(self, t):
                self.parts = [Part(t)]
        class Candidate:
            def __init__(self, t):
                self.content = Content(t)
                self.finish_reason = 'STOP'
            def __getattr__(self, item):
                return None
        self.candidates = [Candidate(text)] if text else []

    def __getattr__(self, item):
        return None

    @property
    def text(self) -> str | None:
        if self._text is Exception:
            raise ValueError("no parts")
        return self._text

class FakeModels:
    def __init__(self, client):
        self._client = client
    def generate_content(self, model, contents, config=None):
        self._client.calls.append({"model": model, "contents": contents})
        item = self._client.responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return item

class FakeFiles:
    def __init__(self, client):
        self._client = client
    def upload(self, file, config=None):
        self._client.files_uploaded.append((file, config))
        class FakeFile:
            name = "test_file"
            uri = "gs://test_file"
        return FakeFile()
    def delete(self, name):
        self._client.files_deleted.append(name)

class FakeClient:
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []
        self.files_uploaded = []
        self.files_deleted = []
        self.models = FakeModels(self)
        self.files = FakeFiles(self)
        self.vertexai = None
        
        class FakeAioModels:
            async def generate_content(inner_self, model, contents, config=None):
                return self.models.generate_content(model, contents, config)
                
        class FakeAio:
            def __init__(self):
                self.models = FakeAioModels()
                
        self.aio = FakeAio()
        self.vertexai = None

class ScriptedBackend:
    def __init__(self, answer="canned", secrets=(), boom=None):
        self.answer = answer
        self.secrets = tuple(secrets)
        self.boom = boom
        self.calls = []

    def extract(self, file_path: str) -> str:
        self.calls.append(("extract", file_path))
        if self.boom is not None:
            raise self.boom
        return self.answer
