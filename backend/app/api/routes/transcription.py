"""分段转写接口。"""

from __future__ import annotations

from fastapi import APIRouter, Depends, File, Form, UploadFile

from app.api.deps import get_api_key, get_provider, get_upstream_base_url
from app.core.config import Settings, get_settings
from app.providers.base import UpstreamProvider
from app.schemas.transcription import TranscriptionResponse
from app.services.transcription import transcribe_segment

router = APIRouter(prefix="/v1/segments", tags=["transcription"])


@router.post("/transcribe", response_model=TranscriptionResponse)
async def transcribe(
    file: UploadFile = File(..., description="音频分段（当前仅支持 WAV）"),
    session_id: str | None = Form(default=None, description="App 侧会话 id，原样回传"),
    seq: int | None = Form(default=None, description="分段序号，原样回传"),
    model: str | None = Form(default=None, description="ASR 模型 id，默认取服务端配置"),
    language: str | None = Form(default=None, description="语言提示（可选）"),
    prompt: str | None = Form(default=None, description="热词/上下文提示（可选，逗号分隔）"),
    api_key: str = Depends(get_api_key),
    provider: UpstreamProvider = Depends(get_provider),
    settings: Settings = Depends(get_settings),
    base_url: str | None = Depends(get_upstream_base_url),
) -> TranscriptionResponse:
    data = await file.read()
    return await transcribe_segment(
        provider=provider,
        settings=settings,
        api_key=api_key,
        filename=file.filename or "segment.wav",
        content_type=file.content_type,
        data=data,
        model=model,
        language=language,
        prompt=prompt,
        session_id=session_id,
        seq=seq,
        base_url=base_url,
    )
