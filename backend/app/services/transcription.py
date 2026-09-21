"""分段转写服务（无状态：音频只在内存中流转，不落盘）。"""

from __future__ import annotations

from app.core.config import Settings
from app.core.errors import PayloadTooLargeError
from app.providers.base import SpeechToTextProvider
from app.schemas.transcription import TranscriptionResponse
from app.services.audio import ensure_supported_audio, wav_duration_sec


async def transcribe_segment(
    *,
    provider: SpeechToTextProvider,
    settings: Settings,
    api_key: str | None,
    filename: str,
    content_type: str | None,
    data: bytes,
    model: str | None = None,
    language: str | None = None,
    prompt: str | None = None,
    session_id: str | None = None,
    seq: int | None = None,
    base_url: str | None = None,
) -> TranscriptionResponse:
    """校验并转写一个音频分段。

    - 输入校验（格式/大小）在此完成，错误信息对客户端友好；
    - 实际识别委托给 :class:`~app.providers.base.SpeechToTextProvider`；
    - ``session_id``/``seq`` 仅回传，服务端不保存任何会话状态。
    """

    limit_bytes = int(settings.max_audio_mb * 1024 * 1024)
    if len(data) > limit_bytes:
        raise PayloadTooLargeError(
            f"音频分段过大：{len(data) / 1024 / 1024:.1f}MB，上限 {settings.max_audio_mb:.0f}MB"
        )

    ensure_supported_audio(filename, content_type, data)

    result = await provider.transcribe(
        data=data,
        filename=filename or "segment.wav",
        content_type=content_type or "audio/wav",
        model=model or settings.default_asr_model,
        api_key=api_key,
        language=language,
        prompt=prompt,
        base_url=base_url,
    )

    return TranscriptionResponse(
        session_id=session_id,
        seq=seq,
        text=result.text,
        # 本地 WAV 解析更精确（秒级浮点）；上游 usage 秒数作为兜底
        duration_sec=wav_duration_sec(data) or result.duration_sec,
        model=result.model,
    )
