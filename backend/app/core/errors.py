"""统一错误体系。

- 业务/输入错误：直接抛出 :class:`AppError` 子类，由全局处理器转成统一 JSON。
- 上游错误：一律包装成 :class:`UpstreamError` / :class:`UpstreamTimeout`，
  避免把 httpx 细节泄漏到 API 层。
"""

from __future__ import annotations

from typing import Any


class AppError(Exception):
    """应用异常基类。"""

    status_code: int = 500
    code: str = "internal_error"

    def __init__(self, message: str, *, details: Any | None = None) -> None:
        super().__init__(message)
        self.message = message
        self.details = details


class BadRequestError(AppError):
    status_code = 400
    code = "bad_request"


class UnauthorizedError(AppError):
    status_code = 401
    code = "unauthorized"


class PayloadTooLargeError(AppError):
    status_code = 413
    code = "payload_too_large"


class UnsupportedMediaError(AppError):
    status_code = 415
    code = "unsupported_media_type"


class UpstreamError(AppError):
    """上游网关/模型返回错误。"""

    status_code = 502
    code = "upstream_error"

    def __init__(
        self,
        message: str,
        *,
        upstream_status: int | None = None,
        retryable: bool = False,
        details: Any | None = None,
    ) -> None:
        super().__init__(message, details=details)
        self.upstream_status = upstream_status
        self.retryable = retryable


class UpstreamTimeoutError(UpstreamError):
    status_code = 504
    code = "upstream_timeout"


class UpstreamInvalidResponseError(UpstreamError):
    """上游响应无法解析（例如缺字段、不是合法 JSON）。"""

    code = "upstream_invalid_response"
