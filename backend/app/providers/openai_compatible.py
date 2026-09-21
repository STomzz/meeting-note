"""OpenAI 兼容上游实现（BNUAPI / new-api / vLLM 通用）。

职责边界：
- 只负责 HTTP 交互、重试与错误归一化；
- 不做任何业务判断（音频校验、提示词、纪要解析都在 service 层）。
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

import httpx

from app.core.config import Settings
from app.core.errors import (
    UnauthorizedError,
    UpstreamError,
    UpstreamInvalidResponseError,
    UpstreamTimeoutError,
)
from app.providers.base import ChatResult, TranscriptionResult

logger = logging.getLogger(__name__)

_RETRYABLE_STATUS = {408, 409, 425, 429, 500, 502, 503, 504}


class OpenAICompatibleProvider:
    """通过 OpenAI 兼容协议访问上游网关。"""

    name = "openai_compatible"

    def __init__(self, settings: Settings) -> None:
        self._settings = settings
        self._base_url = settings.upstream_base_url.rstrip("/")
        self._client: httpx.AsyncClient | None = None

    # ------------------------------------------------------------------ 基础
    def _get_client(self) -> httpx.AsyncClient:
        if self._client is None:
            timeout = httpx.Timeout(
                timeout=self._settings.upstream_timeout_s,
                connect=self._settings.upstream_connect_timeout_s,
            )
            limits = httpx.Limits(max_connections=64, max_keepalive_connections=16)
            self._client = httpx.AsyncClient(timeout=timeout, limits=limits)
        return self._client

    def _resolve_base_url(self, override: str | None) -> str:
        """请求级覆盖上游地址（已由 policy 层校验）；未提供则用服务端配置。"""

        return (override or self._base_url).rstrip("/")

    async def aclose(self) -> None:
        if self._client is not None:
            await self._client.aclose()
            self._client = None

    @staticmethod
    def _auth_headers(api_key: str | None) -> dict[str, str]:
        headers = {"Accept": "application/json"}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        return headers

    @staticmethod
    def _extract_error_message(response: httpx.Response) -> str:
        try:
            payload = response.json()
        except Exception:  # noqa: BLE001 - 上游可能返回非 JSON
            return response.text[:500] or f"HTTP {response.status_code}"
        if isinstance(payload, dict):
            error = payload.get("error")
            if isinstance(error, dict) and error.get("message"):
                return str(error["message"])
            if isinstance(error, str):
                return error
            for key in ("message", "detail", "msg"):
                if payload.get(key):
                    return str(payload[key])
        return json.dumps(payload, ensure_ascii=False)[:500]

    async def _request_with_retry(
        self,
        method: str,
        url: str,
        *,
        headers: dict[str, str],
        kwargs: dict[str, Any],
        context: str,
    ) -> httpx.Response:
        """带指数退避的请求；对 429/5xx/网络错误重试。"""

        attempts = max(1, self._settings.upstream_max_retries + 1)
        last_error: Exception | None = None

        for attempt in range(1, attempts + 1):
            try:
                response = await self._get_client().request(method, url, headers=headers, **kwargs)
            except httpx.TimeoutException as exc:
                last_error = exc
                if attempt < attempts:
                    await asyncio.sleep(min(2 ** (attempt - 1), 4))
                    continue
                raise UpstreamTimeoutError(f"上游超时（{context}）") from exc
            except httpx.HTTPError as exc:
                last_error = exc
                if attempt < attempts:
                    await asyncio.sleep(min(2 ** (attempt - 1), 4))
                    continue
                raise UpstreamError(f"无法连接上游（{context}）：{exc}") from exc

            if response.status_code < 400:
                return response

            message = self._extract_error_message(response)
            if response.status_code in (401, 403):
                # Key 无效/无权限：不重试，直接归一化成 401，让 App 提示用户改 Key
                raise UnauthorizedError(
                    f"上游鉴权失败（{context}）：{message}",
                    details={"upstream_status": response.status_code},
                )
            retryable = response.status_code in _RETRYABLE_STATUS
            if retryable and attempt < attempts:
                logger.warning(
                    "上游 %s 返回 %s，第 %s/%s 次重试：%s",
                    context,
                    response.status_code,
                    attempt,
                    attempts,
                    message,
                )
                await asyncio.sleep(min(2 ** (attempt - 1), 4))
                continue

            raise UpstreamError(
                f"上游返回 {response.status_code}（{context}）：{message}",
                upstream_status=response.status_code,
                retryable=retryable,
            )

        raise UpstreamError(f"上游请求失败（{context}）：{last_error}")

    # ---------------------------------------------------------------- Models
    async def list_models(
        self, *, api_key: str | None = None, base_url: str | None = None
    ) -> list[str]:
        """列出上游可用模型（用于 App 设置页的「测试连接」）。"""

        url = f"{self._resolve_base_url(base_url)}/models"
        response = await self._request_with_retry(
            "GET",
            url,
            headers=self._auth_headers(api_key),
            kwargs={},
            context="models",
        )

        try:
            payload = response.json()
        except Exception as exc:  # noqa: BLE001
            raise UpstreamInvalidResponseError("上游返回非 JSON 响应（models）") from exc

        data = payload.get("data") if isinstance(payload, dict) else None
        if not isinstance(data, list):
            raise UpstreamInvalidResponseError("上游 models 响应缺少 data 字段", details=payload)

        return [str(item["id"]) for item in data if isinstance(item, dict) and item.get("id")]

    # ------------------------------------------------------------------- STT
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
    ) -> TranscriptionResult:
        url = f"{self._resolve_base_url(base_url)}/audio/transcriptions"
        form: dict[str, str] = {"model": model, "response_format": "json"}
        if language:
            form["language"] = language
        if prompt:
            form["prompt"] = prompt

        files = {"file": (filename or "segment.wav", data, content_type or "audio/wav")}
        response = await self._request_with_retry(
            "POST",
            url,
            headers=self._auth_headers(api_key),
            kwargs={"data": form, "files": files},
            context="audio/transcriptions",
        )

        try:
            payload = response.json()
        except Exception as exc:  # noqa: BLE001
            raise UpstreamInvalidResponseError("ASR 上游返回非 JSON 响应") from exc

        text = payload.get("text") if isinstance(payload, dict) else None
        if not isinstance(text, str):
            raise UpstreamInvalidResponseError("ASR 上游响应缺少 text 字段", details=payload)

        usage = payload.get("usage") if isinstance(payload, dict) else None
        duration = None
        if isinstance(usage, dict) and isinstance(usage.get("seconds"), (int, float)):
            duration = float(usage["seconds"])

        return TranscriptionResult(
            text=text,
            model=str(payload.get("model") or model),
            duration_sec=duration,
            raw_usage=usage if isinstance(usage, dict) else {},
        )

    # ------------------------------------------------------------------ Chat
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
    ) -> ChatResult:
        url = f"{self._resolve_base_url(base_url)}/chat/completions"
        body: dict[str, Any] = {
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "stream": False,
        }
        if max_tokens:
            body["max_tokens"] = max_tokens
        if response_format:
            body["response_format"] = response_format

        response = await self._request_with_retry(
            "POST",
            url,
            headers=self._auth_headers(api_key),
            kwargs={"json": body},
            context="chat/completions",
        )

        try:
            payload = response.json()
        except Exception as exc:  # noqa: BLE001
            raise UpstreamInvalidResponseError("LLM 上游返回非 JSON 响应") from exc

        try:
            choice = payload["choices"][0]
            content = choice["message"]["content"] or ""
        except (KeyError, IndexError, TypeError) as exc:
            raise UpstreamInvalidResponseError("LLM 上游响应结构异常", details=payload) from exc

        return ChatResult(
            content=content,
            model=str(payload.get("model") or model),
            usage=payload.get("usage") or {},
        )
