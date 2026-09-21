"""应用配置。

所有可调参数集中于此，全部来自环境变量（前缀 ``MMA_``）/ ``.env``。
上层代码只依赖 :class:`Settings`，禁止在业务代码里硬编码上游地址、密钥或模型名。
"""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict

PACKAGE_DIR = Path(__file__).resolve().parent.parent


class Settings(BaseSettings):
    """运行时配置。"""

    model_config = SettingsConfigDict(
        env_prefix="MMA_",
        env_file=".env",
        env_file_encoding="utf-8",
        extra="ignore",
    )

    # --- 服务 ---
    app_name: str = "bnu-meeting-api"
    version: str = "0.1.0"
    host: str = "0.0.0.0"
    port: int = 8000
    log_level: str = "INFO"

    # --- 上游：OpenAI 兼容网关（默认 BNUAPI）---
    # provider 名称留给未来扩展（例如直连 vLLM 的实现），当前只有 openai_compatible
    upstream_provider: str = "openai_compatible"
    upstream_base_url: str = "https://chatapi.bnu.edu.cn/v1"
    # 客户端可通过 X-Upstream-Base-URL 覆盖上游；下列主机白名单为空=不限制
    # 支持精确主机名，或以 . 开头表示域名后缀（如 ".bnu.edu.cn"）
    upstream_allowed_hosts: str = "chatapi.bnu.edu.cn,.bnu.edu.cn"
    upstream_api_key: str = ""
    allow_server_key: bool = True
    upstream_timeout_s: float = 300.0
    upstream_connect_timeout_s: float = 10.0
    upstream_max_retries: int = 2

    # --- 默认模型（App 可逐请求覆盖）---
    default_asr_model: str = "qwen3-asr-1.7b"
    default_minutes_model: str = "Qwen-Inno-35B-v1"
    minutes_temperature: float = 0.2
    minutes_max_tokens: int = 8192
    use_json_response_format: bool = True
    # 超过该字数走 map-reduce（分段抽取 + 合并）流程
    minutes_map_reduce_threshold_chars: int = 60_000
    # map 阶段每段的字数
    minutes_chunk_chars: int = 20_000

    # --- 输入限制 ---
    max_audio_mb: float = 50.0
    max_transcript_chars: int = 1_500_000

    # --- 提示词目录（外置，便于维护/覆写）---
    prompts_dir: Path = PACKAGE_DIR / "prompts"

    # --- 音频转码（可选；默认关闭，App 直传 WAV 即可）---
    transcode_enabled: bool = False
    ffmpeg_bin: str = "ffmpeg"


@lru_cache
def get_settings() -> Settings:
    """返回进程级单例配置（依赖注入用）。"""

    return Settings()
