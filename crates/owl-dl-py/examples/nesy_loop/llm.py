from __future__ import annotations
from typing import Protocol


class LLM(Protocol):
    def propose(self, prompt: str) -> str: ...


def _first_text_block(content) -> str:
    for b in content:
        if getattr(b, "type", None) == "text":
            return b.text
    raise ValueError("no text block in Anthropic response")


class ScriptedLLM:
    """Deterministic LLM stub: returns canned replies in order."""
    def __init__(self, replies: list[str]):
        self._replies = list(replies)
        self._i = 0

    def propose(self, prompt: str) -> str:
        try:
            reply = self._replies[self._i]
        except IndexError:
            raise IndexError(f"ScriptedLLM: replies exhausted after {self._i} call(s)")
        self._i += 1
        return reply


def _openai_content(resp) -> str:
    """Extract the assistant text from an OpenAI-compatible chat response."""
    return resp["choices"][0]["message"]["content"].strip()


class LocalLLM:
    """A local, OpenAI-compatible chat endpoint (default: Ollama at :11434).

    Uses only the stdlib (urllib) so the demo runs fully offline with no extra
    dependency and no API key.
    """
    def __init__(self, model: str = "qwen2.5:32b-instruct",
                 base_url: str = "http://localhost:11434/v1",
                 temperature: float = 0.4):
        self._model = model
        self._url = base_url.rstrip("/") + "/chat/completions"
        self._temperature = temperature

    def propose(self, prompt: str) -> str:
        import json
        import urllib.request
        body = json.dumps({
            "model": self._model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": self._temperature,
            "stream": False,
        }).encode()
        req = urllib.request.Request(
            self._url, data=body, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req) as r:
            return _openai_content(json.load(r))


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
        return _first_text_block(resp.content).strip()
