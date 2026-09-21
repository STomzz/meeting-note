"""上游 Provider 工厂。

新增上游实现时：
1. 在 :mod:`app.providers` 下实现 :class:`~app.providers.base.UpstreamProvider`；
2. 在此注册名字；
3. 通过环境变量 ``MMA_UPSTREAM_PROVIDER`` 切换，业务代码零改动。
"""

from __future__ import annotations

from app.core.config import Settings
from app.core.errors import AppError
from app.providers.base import UpstreamProvider
from app.providers.openai_compatible import OpenAICompatibleProvider

_REGISTRY = {
    OpenAICompatibleProvider.name: OpenAICompatibleProvider,
}


def create_provider(settings: Settings) -> UpstreamProvider:
    provider_cls = _REGISTRY.get(settings.upstream_provider)
    if provider_cls is None:
        raise AppError(
            f"未注册的上游 provider：{settings.upstream_provider}"
            f"（可选：{', '.join(sorted(_REGISTRY))}）"
        )
    return provider_cls(settings)
