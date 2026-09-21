"""应用装配入口。

- :func:`create_app` 为工厂函数，便于测试与多实例部署；
- 上游 provider 与提示词环境在 lifespan 中构建，通过 ``app.state`` 注入依赖。
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager
from uuid import uuid4

from fastapi import FastAPI, Request

from app.api.exception_handlers import register_exception_handlers
from app.api.routes import export, health, minutes, transcription
from app.core.config import Settings, get_settings
from app.core.logging import setup_logging
from app.core.prompts import create_prompts_env
from app.providers.factory import create_provider
from app.services.minutes import MinutesService

logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings: Settings = app.state.settings

    provider = create_provider(settings)
    prompts = create_prompts_env(settings.prompts_dir)

    app.state.provider = provider
    app.state.minutes_service = MinutesService(provider, settings, prompts)

    logger.info(
        "%s %s 启动（provider=%s, upstream=%s, default_asr=%s, default_minutes=%s）",
        settings.app_name,
        settings.version,
        provider.name,
        settings.upstream_base_url,
        settings.default_asr_model,
        settings.default_minutes_model,
    )
    try:
        yield
    finally:
        await provider.aclose()
        logger.info("上游连接已关闭")


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    setup_logging(settings.log_level)

    app = FastAPI(
        title=settings.app_name,
        version=settings.version,
        description="BNU 会议纪要后端：分段转写 + 纪要生成 + Word 导出（无状态）",
        lifespan=lifespan,
    )
    app.state.settings = settings

    @app.middleware("http")
    async def request_id_middleware(request: Request, call_next):
        request_id = request.headers.get("X-Request-ID") or uuid4().hex[:16]
        request.state.request_id = request_id
        response = await call_next(request)
        response.headers["X-Request-ID"] = request_id
        return response

    register_exception_handlers(app)

    app.include_router(health.router)
    app.include_router(transcription.router)
    app.include_router(minutes.router)
    app.include_router(export.router)

    @app.get("/", include_in_schema=False)
    async def root() -> dict:
        return {"name": settings.app_name, "version": settings.version, "docs": "/docs"}

    return app


app = create_app()
