"""测试夹具：假上游 provider + 内存 ASGI 客户端（不依赖网络）。"""

from __future__ import annotations

import io
import json
import struct
import wave
from collections import deque

import httpx
import pytest

from app.core.config import Settings, get_settings
from app.core.prompts import create_prompts_env
from app.providers.base import ChatResult, TranscriptionResult
from app.services.minutes import MinutesService

DEFAULT_MINUTES = {
    "title": "产品周会",
    "meeting_date": "2026-09-21",
    "participants": ["张三", "李四"],
    "overview": "本次会议讨论了产品进度与发布计划。",
    "topics": [{"title": "发布计划", "points": ["下周三灰度", "周五全量"]}],
    "decisions": ["按原计划发布"],
    "action_items": [
        {"task": "准备发布公告", "owner": "李四", "due": "周三", "source": "李四：我来写公告"}
    ],
    "risks": ["灰度期间监控不足"],
    "next_steps": ["下周一预演"],
}


def make_wav_bytes(duration_sec: float = 1.0, sample_rate: int = 16000) -> bytes:
    """生成一段静音 WAV（用于接口测试）。"""

    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(sample_rate)
        handle.writeframes(struct.pack("<h", 0) * int(duration_sec * sample_rate))
    return buffer.getvalue()


class FakeProvider:
    """内存假上游：记录调用参数并返回可编排的响应。"""

    name = "fake"

    def __init__(self) -> None:
        self.transcribe_calls: list[dict] = []
        self.chat_calls: list[dict] = []
        self.model_calls: dict = {}
        self.transcribe_result = TranscriptionResult(
            text="这是测试转写文本。", model="qwen3-asr-1.7b", duration_sec=1.0
        )
        self.chat_responses: deque[str] = deque()

    async def transcribe(self, **kwargs) -> TranscriptionResult:
        self.transcribe_calls.append(kwargs)
        return self.transcribe_result

    async def complete(self, **kwargs) -> ChatResult:
        self.chat_calls.append(kwargs)
        content = (
            self.chat_responses.popleft()
            if self.chat_responses
            else json.dumps(DEFAULT_MINUTES, ensure_ascii=False)
        )
        return ChatResult(
            content=content, model=kwargs.get("model", "fake-model"), usage={"total_tokens": 42}
        )

    async def list_models(self, **kwargs) -> list[str]:
        self.model_calls = kwargs
        return ["qwen3-asr-1.7b", "qwen3.8-27b"]

    async def aclose(self) -> None:  # pragma: no cover - 测试用空实现
        return None


@pytest.fixture
def settings(tmp_path) -> Settings:
    return Settings(
        _env_file=None,
        upstream_base_url="http://upstream.test/v1",
        upstream_api_key="server-test-key",
        allow_server_key=True,
        prompts_dir=Settings.model_fields["prompts_dir"].default,
    )


@pytest.fixture
def fake_provider() -> FakeProvider:
    return FakeProvider()


@pytest.fixture
async def client(settings: Settings, fake_provider: FakeProvider):
    from app.main import create_app

    app = create_app(settings)
    app.state.settings = settings
    app.state.provider = fake_provider
    app.state.minutes_service = MinutesService(
        fake_provider, settings, create_prompts_env(settings.prompts_dir)
    )
    app.dependency_overrides[get_settings] = lambda: settings

    transport = httpx.ASGITransport(app=app)
    async with httpx.AsyncClient(transport=transport, base_url="http://test") as http_client:
        yield http_client
