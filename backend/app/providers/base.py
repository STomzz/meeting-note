"""上游能力抽象（解耦核心）。

API/Service 层只依赖这里的 Protocol，不依赖任何具体 SDK/HTTP 细节；
更换上游（BNUAPI ↔ 直连 vLLM ↔ 其他 OpenAI 兼容服务）只需新增实现 +
在 :mod:`app.providers.factory` 注册。
"""

from __future__ import annotations

from typing import Protocol

from pydantic import BaseModel, Field


class TranscriptionResult(BaseModel):
    text: str
    model: str
    duration_sec: float | None = None
    raw_usage: dict = Field(default_factory=dict)


class ChatResult(BaseModel):
    content: str
    model: str
    usage: dict = Field(default_factory=dict)


class SpeechToTextProvider(Protocol):
    """语音转文字能力。"""

    async def transcribe(
        self,
        *,
        data: bytes,
        filename: str,
        content_type: str,
        model: str,
        api_key: str | None = None,
        language: str | None = None,
        prompt: str | None = None,
        base_url: str | None = None,
    ) -> TranscriptionResult: ...


class ChatProvider(Protocol):
    """对话补全能力。"""

    async def complete(
        self,
        *,
        messages: list[dict],
        model: str,
        api_key: str | None = None,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        response_format: dict | None = None,
        base_url: str | None = None,
    ) -> ChatResult: ...


class UpstreamProvider(SpeechToTextProvider, ChatProvider, Protocol):
    """上游网关需要同时提供 STT 与 Chat 能力。"""

    name: str

    async def list_models(
        self, *, api_key: str | None = None, base_url: str | None = None
    ) -> list[str]: ...

    async def aclose(self) -> None: ...
