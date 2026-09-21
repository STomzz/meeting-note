"""公共响应模型。"""

from __future__ import annotations

from pydantic import BaseModel, Field


class HealthResponse(BaseModel):
    status: str = "ok"
    app: str
    version: str
    provider: str
    upstream_base_url: str


class ErrorBody(BaseModel):
    code: str
    message: str
    request_id: str | None = None
    details: object | None = None


class ErrorResponse(BaseModel):
    error: ErrorBody


class ModelInfo(BaseModel):
    """默认模型信息，便于 App 做默认值提示。"""

    asr: str
    minutes: str


class UsageInfo(BaseModel):
    prompt_tokens: int | None = None
    completion_tokens: int | None = None
    total_tokens: int | None = None
    extra: dict = Field(default_factory=dict)


class UpstreamModelsResponse(BaseModel):
    """上游可用模型列表。"""

    models: list[str] = Field(default_factory=list)
