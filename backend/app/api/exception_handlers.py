"""统一异常处理：所有错误都返回 ``{"error": {...}}`` 结构，便于客户端处理。"""

from __future__ import annotations

import logging

from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse

from app.core.errors import AppError

logger = logging.getLogger(__name__)


def _request_id(request: Request) -> str | None:
    return getattr(request.state, "request_id", None)


def _error_response(request: Request, status_code: int, code: str, message: str, details=None):
    return JSONResponse(
        status_code=status_code,
        content={
            "error": {
                "code": code,
                "message": message,
                "request_id": _request_id(request),
                "details": details,
            }
        },
    )


def register_exception_handlers(app: FastAPI) -> None:
    @app.exception_handler(AppError)
    async def handle_app_error(request: Request, exc: AppError):
        if exc.status_code >= 500:
            logger.warning("业务异常 %s：%s", exc.code, exc.message)
        return _error_response(request, exc.status_code, exc.code, exc.message, exc.details)

    @app.exception_handler(RequestValidationError)
    async def handle_validation_error(request: Request, exc: RequestValidationError):
        return _error_response(
            request,
            422,
            "validation_error",
            "请求参数不合法",
            details=exc.errors(),
        )

    @app.exception_handler(Exception)
    async def handle_unexpected(request: Request, exc: Exception):
        logger.exception("未捕获异常：%s", exc)
        return _error_response(request, 500, "internal_error", "服务内部错误")
