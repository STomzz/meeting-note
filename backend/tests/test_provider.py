"""Provider 层测试：错误归一化与响应解析（用 httpx MockTransport，不依赖网络）。"""

from __future__ import annotations

import httpx
import pytest

from app.core.config import Settings
from app.core.errors import UnauthorizedError, UpstreamError, UpstreamInvalidResponseError
from app.providers.openai_compatible import OpenAICompatibleProvider


def make_provider(handler, **overrides) -> OpenAICompatibleProvider:
    settings = Settings(
        _env_file=None,
        upstream_base_url="http://upstream.test/v1",
        **{"upstream_max_retries": 0, **overrides},
    )
    provider = OpenAICompatibleProvider(settings)
    provider._client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    return provider


async def test_transcribe_parses_text_and_duration():
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path.endswith("/audio/transcriptions")
        assert request.headers["authorization"] == "Bearer user-key"
        return httpx.Response(
            200, json={"text": "你好", "model": "qwen3-asr-1.7b", "usage": {"seconds": 3.5}}
        )

    provider = make_provider(handler)
    result = await provider.transcribe(
        data=b"RIFF",
        filename="a.wav",
        content_type="audio/wav",
        model="qwen3-asr-1.7b",
        api_key="user-key",
    )
    assert result.text == "你好"
    assert result.duration_sec == 3.5


async def test_transcribe_maps_upstream_error():
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            400, json={"error": {"message": "Invalid or unsupported audio file."}}
        )

    provider = make_provider(handler)
    with pytest.raises(UpstreamError) as excinfo:
        await provider.transcribe(
            data=b"RIFF", filename="a.wav", content_type="audio/wav", model="m", api_key=None
        )
    assert excinfo.value.upstream_status == 400
    assert "unsupported audio" in excinfo.value.message


async def test_transcribe_invalid_response():
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"unexpected": True})

    provider = make_provider(handler)
    with pytest.raises(UpstreamInvalidResponseError):
        await provider.transcribe(
            data=b"RIFF", filename="a.wav", content_type="audio/wav", model="m", api_key=None
        )


async def test_complete_parses_choice():
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            json={
                "model": "qwen3.8-27b",
                "choices": [{"message": {"content": "{}"}}],
                "usage": {"total_tokens": 10},
            },
        )

    provider = make_provider(handler)
    result = await provider.complete(
        messages=[{"role": "user", "content": "hi"}], model="qwen3.8-27b"
    )
    assert result.content == "{}"
    assert result.usage["total_tokens"] == 10


async def test_retry_on_503_then_success():
    calls = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        calls["count"] += 1
        if calls["count"] == 1:
            return httpx.Response(503, json={"error": {"message": "overloaded"}})
        return httpx.Response(200, json={"model": "m", "choices": [{"message": {"content": "ok"}}]})

    provider = make_provider(handler, upstream_max_retries=1)
    result = await provider.complete(messages=[{"role": "user", "content": "hi"}], model="m")
    assert result.content == "ok"
    assert calls["count"] == 2


async def test_models_lists_ids():
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path.endswith("/models")
        assert request.headers["authorization"] == "Bearer user-key"
        return httpx.Response(
            200, json={"data": [{"id": "qwen3-asr-1.7b"}, {"id": "qwen3.8-27b"}, {"bad": 1}]}
        )

    provider = make_provider(handler)
    models = await provider.list_models(api_key="user-key")
    assert models == ["qwen3-asr-1.7b", "qwen3.8-27b"]


async def test_401_becomes_unauthorized_without_retry():
    calls = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        calls["count"] += 1
        return httpx.Response(401, json={"error": {"message": "无效的令牌"}})

    provider = make_provider(handler, upstream_max_retries=2)
    with pytest.raises(UnauthorizedError) as excinfo:
        await provider.complete(messages=[{"role": "user", "content": "hi"}], model="m")
    assert calls["count"] == 1
    assert "无效的令牌" in excinfo.value.message
