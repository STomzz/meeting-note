"""FastAPI 依赖：鉴权、上游 provider、纪要服务。"""

from __future__ import annotations

from fastapi import Depends, Header, Request

from app.core.config import Settings, get_settings
from app.core.errors import UnauthorizedError
from app.core.policy import validate_upstream_base_url
from app.providers.base import UpstreamProvider
from app.services.minutes import MinutesService


def get_provider(request: Request) -> UpstreamProvider:
    return request.app.state.provider


def get_minutes_service(request: Request) -> MinutesService:
    return request.app.state.minutes_service


def get_upstream_base_url(
    x_upstream_base_url: str | None = Header(default=None, alias="X-Upstream-Base-URL"),
    settings: Settings = Depends(get_settings),
) -> str | None:
    """可选：请求级覆盖上游地址（App 设置里的 BNUAPI 地址）。

    未提供则使用服务端配置 ``MMA_UPSTREAM_BASE_URL``。
    """

    if not x_upstream_base_url:
        return None
    return validate_upstream_base_url(x_upstream_base_url, settings)


def get_api_key(
    authorization: str | None = Header(default=None),
    settings: Settings = Depends(get_settings),
) -> str:
    """取客户端携带的 BNUAPI Key。

    约定：``Authorization: Bearer <BNUAPI key>``。
    本地开发可用 ``MMA_UPSTREAM_API_KEY`` 作为兜底（``MMA_ALLOW_SERVER_KEY=true``）。
    """

    token: str | None = None
    if authorization:
        parts = authorization.split(" ", 1)
        token = (
            parts[1].strip()
            if len(parts) == 2 and parts[0].lower() == "bearer"
            else authorization.strip()
        )

    if token:
        return token

    if settings.allow_server_key and settings.upstream_api_key:
        return settings.upstream_api_key

    raise UnauthorizedError(
        "缺少 API Key：请在 App 设置中填写 BNUAPI Key（请求头 Authorization: Bearer <key>）"
    )
