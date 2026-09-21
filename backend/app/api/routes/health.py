"""健康检查与默认配置。"""

from __future__ import annotations

from fastapi import APIRouter, Depends

from app.api.deps import get_api_key, get_provider, get_upstream_base_url
from app.core.config import Settings, get_settings
from app.providers.base import UpstreamProvider
from app.schemas.common import HealthResponse, ModelInfo, UpstreamModelsResponse

router = APIRouter(tags=["health"])


@router.get("/health", response_model=HealthResponse)
async def health(
    settings: Settings = Depends(get_settings),
    provider: UpstreamProvider = Depends(get_provider),
) -> HealthResponse:
    return HealthResponse(
        status="ok",
        app=settings.app_name,
        version=settings.version,
        provider=provider.name,
        upstream_base_url=settings.upstream_base_url,
    )


@router.get("/v1/defaults", response_model=ModelInfo)
async def defaults(settings: Settings = Depends(get_settings)) -> ModelInfo:
    """App 启动时可拉取默认模型 id（用户仍可自由覆盖）。"""

    return ModelInfo(asr=settings.default_asr_model, minutes=settings.default_minutes_model)


@router.get("/v1/upstream/models", response_model=UpstreamModelsResponse)
async def upstream_models(
    api_key: str = Depends(get_api_key),
    provider: UpstreamProvider = Depends(get_provider),
    base_url: str | None = Depends(get_upstream_base_url),
) -> UpstreamModelsResponse:
    """用当前 Key 拉取上游模型列表（App 设置页「测试连接」用，不消耗模型额度）。"""

    models = await provider.list_models(api_key=api_key, base_url=base_url)
    return UpstreamModelsResponse(models=models)
