"""分段转写接口测试。"""

from __future__ import annotations

from tests.conftest import make_wav_bytes


async def test_transcribe_wav_success(client, fake_provider):
    wav = make_wav_bytes(duration_sec=2.0)
    response = await client.post(
        "/v1/segments/transcribe",
        files={"file": ("seg1.wav", wav, "audio/wav")},
        data={"session_id": "sess-1", "seq": "3", "model": "qwen3-asr-1.7b", "prompt": "张三,李四"},
        headers={"Authorization": "Bearer user-key"},
    )

    assert response.status_code == 200
    payload = response.json()
    assert payload["text"] == "这是测试转写文本。"
    assert payload["session_id"] == "sess-1"
    assert payload["seq"] == 3
    assert payload["duration_sec"] == 2.0  # 来自本地 WAV 解析

    call = fake_provider.transcribe_calls[0]
    assert call["api_key"] == "user-key"  # 用户 key 透传
    assert call["model"] == "qwen3-asr-1.7b"
    assert call["prompt"] == "张三,李四"


async def test_transcribe_rejects_non_wav(client):
    response = await client.post(
        "/v1/segments/transcribe",
        files={"file": ("seg1.mp3", b"ID3\x04fake-mp3-bytes", "audio/mpeg")},
        headers={"Authorization": "Bearer user-key"},
    )
    assert response.status_code == 415
    assert response.json()["error"]["code"] == "unsupported_media_type"


async def test_transcribe_rejects_empty_file(client):
    response = await client.post(
        "/v1/segments/transcribe",
        files={"file": ("seg1.wav", b"", "audio/wav")},
        headers={"Authorization": "Bearer user-key"},
    )
    assert response.status_code == 400


async def test_transcribe_requires_api_key(settings, fake_provider):
    import httpx

    from app.core.config import get_settings
    from app.core.prompts import create_prompts_env
    from app.main import create_app
    from app.services.minutes import MinutesService

    strict = settings.model_copy(update={"allow_server_key": False, "upstream_api_key": ""})
    app = create_app(strict)
    app.state.provider = fake_provider
    app.state.minutes_service = MinutesService(
        fake_provider, strict, create_prompts_env(strict.prompts_dir)
    )
    app.dependency_overrides[get_settings] = lambda: strict

    transport = httpx.ASGITransport(app=app)
    async with httpx.AsyncClient(transport=transport, base_url="http://test") as http_client:
        response = await http_client.post(
            "/v1/segments/transcribe",
            files={"file": ("seg1.wav", make_wav_bytes(), "audio/wav")},
        )
    assert response.status_code == 401


async def test_server_key_fallback(client, fake_provider):
    response = await client.post(
        "/v1/segments/transcribe",
        files={"file": ("seg1.wav", make_wav_bytes(), "audio/wav")},
    )
    assert response.status_code == 200
    assert fake_provider.transcribe_calls[-1]["api_key"] == "server-test-key"
