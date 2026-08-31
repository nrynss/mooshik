from __future__ import annotations
import json
import logging
from pathlib import Path
from typing import Any, Sequence
from datetime import datetime, timezone
import asyncio

from mooshik_common.concepts import parse_concepts
from google.adk.runners import InMemoryRunner
from google.genai import types
from .config import DEFAULT_MODEL
from .errors import ArtifactsToolError
from .secretscan import find_secret
from .agent import build_agent

log = logging.getLogger(__name__)

class ArtifactsBackend:
    def __init__(self, client: Any, model: str = DEFAULT_MODEL, timeout_secs: float = 45.0, secrets: Sequence[str] = ()):
        self.client = client
        self.model = model
        self.timeout_secs = timeout_secs
        self.secrets = tuple(s for s in secrets if s)

    def extract(self, file_path: str) -> str:
        # extraction tool runs in a separate thread because of the async loop in run().
        # we can use run_async but wait, extract is called in wait_for thread.
        # it's safe to use asyncio.run if it's in a thread. 
        # But wait, guarded() already does to_thread. So we can just use the sync run().
        return asyncio.run(self._extract_async(file_path))

    async def _extract_async(self, file_path: str) -> str:
        path = Path(file_path)
        if not path.is_file():
            raise ArtifactsToolError(f"File not found: {file_path}")
            
        ext = path.suffix.lower()
        if ext in (".png", ".jpg", ".jpeg", ".webp", ".gif"):
            mime_type = f"image/{ext[1:].replace('jpg', 'jpeg')}"
        elif ext in (".wav", ".mp3", ".m4a", ".ogg"):
            mime_type = f"audio/{ext[1:]}"
        else:
            raise ArtifactsToolError(f"Unsupported file type: {ext}")
            
        mtime = path.stat().st_mtime
        event_time = datetime.fromtimestamp(mtime, tz=timezone.utc).isoformat()
        
        try:
            uploaded_file = self.client.files.upload(file=str(path), config={"mime_type": mime_type})
        except Exception as e:
            raise ArtifactsToolError(f"Failed to upload file to Gemini: {e}")
            
        try:
            agent = build_agent(self.client, self.model)
            runner = InMemoryRunner(agent=agent)
            runner.auto_create_session = True
            
            message = types.Content(
                role="user",
                parts=[
                    types.Part.from_text(text="Analyze the following artifact."),
                    types.Part.from_uri(file_uri=uploaded_file.uri, mime_type=mime_type),
                ]
            )
            
            raw_response = ""
            async for event in runner.run_async(user_id="user", session_id="extract", new_message=message):
                if hasattr(event, "content") and event.content:
                    parts = getattr(event.content, "parts", [])
                    for p in parts:
                        if hasattr(p, "text") and p.text:
                            raw_response += p.text
                                
            secret_hit = find_secret(raw_response, self.secrets)
            if secret_hit:
                log.warning("Secret detected (%s) in extraction, dropping document.", secret_hit)
                return json.dumps({"error": "whole-document drop: secret detected", "event_time": event_time})
                
            concepts = parse_concepts(raw_response)
            out = [{"content": c.content, "concept_type": c.concept_type} for c in concepts]
            return json.dumps({"event_time": event_time, "concepts": out})
            
        finally:
            try:
                self.client.files.delete(name=uploaded_file.name)
            except Exception:
                pass

def make_client(settings: Any) -> Any:
    from mooshik_common.vertex import make_client as build
    if not settings.use_vertex: return build(api_key=settings.api_key)
    return build(project=settings.project, location=settings.location, credentials_path=settings.credentials_path)
