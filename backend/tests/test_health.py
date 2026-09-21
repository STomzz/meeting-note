"""健康检查与默认模型接口测试。"""

from __future__ import annotations


async def test_health(client):
    response = await client.get("/health")
    assert response.status_code == 200
    payload = response.json()
    assert payload["status"] == "ok"
    assert payload["provider"] == "fake"
    assert payload["upstream_base_url"] == "http://upstream.test/v1"


async def test_defaults(client):
    response = await client.get("/v1/defaults")
    assert response.status_code == 200
    payload = response.json()
    assert payload["asr"]
    assert payload["minutes"]


async def test_request_id_header(client):
    response = await client.get("/health", headers={"X-Request-ID": "abc123"})
    assert response.headers["X-Request-ID"] == "abc123"
