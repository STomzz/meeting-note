"""上游地址策略测试。"""

from __future__ import annotations

import pytest

from app.core.config import Settings
from app.core.errors import BadRequestError
from app.core.policy import validate_upstream_base_url


def make_settings(**overrides) -> Settings:
    return Settings(_env_file=None, **overrides)


def test_normalizes_and_allows_whitelisted_host():
    settings = make_settings(upstream_allowed_hosts="chatapi.bnu.edu.cn")
    assert (
        validate_upstream_base_url("https://chatapi.bnu.edu.cn/v1/", settings)
        == "https://chatapi.bnu.edu.cn/v1"
    )


def test_suffix_whitelist_matches_subdomains():
    settings = make_settings(upstream_allowed_hosts=".bnu.edu.cn")
    assert validate_upstream_base_url("https://newapi.bnu.edu.cn/v1", settings)
    with pytest.raises(BadRequestError):
        validate_upstream_base_url("https://evil-bnu.edu.cn/v1", settings)


def test_rejects_host_outside_whitelist():
    settings = make_settings(upstream_allowed_hosts="chatapi.bnu.edu.cn")
    with pytest.raises(BadRequestError):
        validate_upstream_base_url("https://evil.example.com/v1", settings)


def test_empty_whitelist_allows_any_host():
    settings = make_settings(upstream_allowed_hosts="")
    assert validate_upstream_base_url("http://127.0.0.1:3000/v1", settings)


@pytest.mark.parametrize("url", ["ftp://chatapi.bnu.edu.cn/v1", "chatapi.bnu.edu.cn/v1", "", "   "])
def test_rejects_invalid_urls(url):
    settings = make_settings(upstream_allowed_hosts="")
    with pytest.raises(BadRequestError):
        validate_upstream_base_url(url, settings)
