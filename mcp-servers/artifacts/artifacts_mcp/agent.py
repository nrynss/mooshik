from __future__ import annotations
from typing import Any
from google.adk.agents import LlmAgent
from google.adk.models import Gemini
from google.genai import Client
from .config import DEFAULT_MODEL

class InjectedGemini(Gemini):
    def __init__(self, model_name: str, client: Client, **kwargs):
        super().__init__(model=model_name, **kwargs)
        self._injected_client = client
        
    @property
    def api_client(self) -> Client:
        return self._injected_client

PROMPT = (
    "You extract durable memory concepts from workspace artifacts (images and audio).\n"
    "Look closely at the provided file. For audio, transcribe what you hear first as a step. "
    "Make a decision about whether anything durable happened.\n"
    "If it did, return ZERO OR MORE concepts worth remembering long-term, as a strict JSON array:\n"
    '[{"content": "<one self-contained concept>", "concept_type": "entity|logic|constraint|resource|observation"}]\n'
    "Rules: Output the JSON array at the end of your response. No duplicates. Skip trivia.\n"
    "Only the five typed concepts are allowed. You MUST explicitly refuse to output:\n"
    "- Descriptions of the artifact as an artifact\n"
    "- UI chrome: window titles, button labels, browser tabs, menu bars\n"
    "- OCR dumps of everything visible\n"
    "- Anything that is not a claim about the workspace\n"
    "Priority: (1) facts with values, (2) structure and relations, (3) identity anchors."
)

def build_agent(client: Client, model: str = DEFAULT_MODEL) -> LlmAgent:
    return LlmAgent(
        name="artifacts_extractor",
        model=InjectedGemini(model_name=model, client=client),
        description="Extracts memory concepts from multimodal non-text artifacts.",
        instruction=PROMPT,
        tools=[],
    )
