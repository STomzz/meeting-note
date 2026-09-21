"""上游地址策略：校验客户端传入的 base URL，避免后端被当成任意代理（SSRF）。"""

from __future__ import annotations

from urllib.parse import urlparse

from app.core.config import Settings
from app.core.errors import BadRequestError

_ALLOWED_SCHEMES = {"http", "https"}


def validate_upstream_base_url(raw_url: str, settings: Settings) -> str:
    """校验并规范化客户端传入的上游 base URL。

    - 只允许 http/https；
    - 若配置了 ``MMA_UPSTREAM_ALLOWED_HOSTS``（逗号分隔，空=不限制），则主机名必须在白名单内。
    """

    candidate = (raw_url or "").strip().rstrip("/")
    if not candidate:
        raise BadRequestError("X-Upstream-Base-URL 不能为空")

    parsed = urlparse(candidate)
    if parsed.scheme not in _ALLOWED_SCHEMES or not parsed.hostname:
        raise BadRequestError(f"X-Upstream-Base-URL 非法：{raw_url!r}（需为 http/https 地址）")

    allowed_hosts = [
        host.strip().lower()
        for host in (settings.upstream_allowed_hosts or "").split(",")
        if host.strip()
    ]
    if allowed_hosts and not _host_allowed(parsed.hostname.lower(), allowed_hosts):
        raise BadRequestError(
            f"X-Upstream-Base-URL 主机 {parsed.hostname!r} 不在允许列表内"
            f"（{', '.join(allowed_hosts)}）"
        )

    return candidate


def _host_allowed(host: str, allowed_hosts: list[str]) -> bool:
    """精确匹配，或以 ``.`` 开头表示域名后缀匹配（例如 ``.bnu.edu.cn``）。"""

    for entry in allowed_hosts:
        if entry.startswith("."):
            if host == entry[1:] or host.endswith(entry):
                return True
        elif host == entry:
            return True
    return False
