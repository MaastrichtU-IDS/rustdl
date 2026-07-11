from __future__ import annotations
from typing import Protocol


class LLM(Protocol):
    def propose(self, prompt: str) -> str: ...


class ScriptedLLM:
    """Deterministic LLM stub: returns canned replies in order."""
    def __init__(self, replies: list[str]):
        self._replies = list(replies)
        self._i = 0

    def propose(self, prompt: str) -> str:
        reply = self._replies[self._i]  # IndexError when exhausted
        self._i += 1
        return reply


class AnthropicLLM:
    def __init__(self, model: str = "claude-sonnet-5"):
        from anthropic import Anthropic
        self._client = Anthropic()  # reads ANTHROPIC_API_KEY
        self._model = model

    def propose(self, prompt: str) -> str:
        resp = self._client.messages.create(
            model=self._model, max_tokens=1024,
            messages=[{"role": "user", "content": prompt}],
        )
        return resp.content[0].text.strip()
