"""请求级上游地址覆盖（App 设置里的 BNUAPI 地址）测试。"""

from __future__ import annotations

from tests.conftest import make_wav_bytes

UPSTREAM = "https://chatapi.bnu.edu.cn/v1"


async def test_transcribe_passes_upstream_override(client, fake_provider):
    response = await client.post(
        "/v1/segments/transcribe",
        files={"file": ("seg.wav", make_wav_bytes(), "audio/wav")},
        headers={"Authorization": "Bearer user-key", "X-Upstream-Base-URL": UPSTREAM},
    )
    assert response.status_code == 200
    assert fake_provider.transcribe_calls[-1]["base_url"] == UPSTREAM


async def test_minutes_passes_upstream_override(client, fake_provider):
    response = await client.post(
        "/v1/minutes",
        json={"transcript": "张三：开始吧。"},
        headers={"Authorization": "Bearer user-key", "X-Upstream-Base-URL": UPSTREAM},
    )
    assert response.status_code == 200
    assert fake_provider.chat_calls[-1]["base_url"] == UPSTREAM


async def test_upstream_override_rejects_unknown_host(client):
    response = await client.post(
        "/v1/minutes",
        json={"transcript": "张三：开始吧。"},
        headers={
            "Authorization": "Bearer user-key",
            "X-Upstream-Base-URL": "https://evil.example.com/v1",
        },
    )
    assert response.status_code == 400
    assert response.json()["error"]["code"] == "bad_request"


async def test_without_override_uses_server_default(client, fake_provider):
    response = await client.post(
        "/v1/minutes",
        json={"transcript": "张三：开始吧。"},
        headers={"Authorization": "Bearer user-key"},
    )
    assert response.status_code == 200
    assert fake_provider.chat_calls[-1]["base_url"] is None


async def test_upstream_models_uses_key_and_override(client, fake_provider):
    response = await client.get(
        "/v1/upstream/models",
        headers={"Authorization": "Bearer user-key", "X-Upstream-Base-URL": UPSTREAM},
    )
    assert response.status_code == 200
    assert response.json()["models"] == ["qwen3-asr-1.7b", "qwen3.8-27b"]
    assert fake_provider.model_calls["api_key"] == "user-key"
    assert fake_provider.model_calls["base_url"] == UPSTREAM


async def test_upstream_models_requires_key(client, settings):
    settings.allow_server_key = False
    settings.upstream_api_key = ""
    response = await client.get("/v1/upstream/models")
    assert response.status_code == 401
