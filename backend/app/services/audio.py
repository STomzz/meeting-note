"""音频处理工具：WAV 校验与时长解析。

设计为纯函数，便于单测；不依赖任何框架。
"""

from __future__ import annotations

import struct

from app.core.errors import BadRequestError, UnsupportedMediaError

_RIFF = b"RIFF"
_WAVE = b"WAVE"


def ensure_supported_audio(filename: str, content_type: str | None, data: bytes) -> None:
    """校验上传音频。

    当前上游 ASR（vLLM 版 Qwen3-ASR）仅支持 WAV，因此这里尽早给出明确错误，
    避免把无法解码的文件转发到上游。
    """

    if not data:
        raise BadRequestError("音频内容为空")

    if not (is_wav(data) or _looks_like_wav_by_name(filename, content_type)):
        raise UnsupportedMediaError(
            "当前仅支持 WAV 音频（16kHz 单声道最佳）。"
            f"收到：filename={filename!r}, content_type={content_type!r}"
        )

    if not is_wav(data):
        raise UnsupportedMediaError(
            "文件不是合法的 WAV（缺少 RIFF/WAVE 头）。请在客户端导出标准 WAV 后再上传。"
        )


def _looks_like_wav_by_name(filename: str, content_type: str | None) -> bool:
    name = (filename or "").lower()
    ctype = (content_type or "").lower()
    return name.endswith(".wav") or ctype in {"audio/wav", "audio/x-wav", "audio/wave"}


def is_wav(data: bytes) -> bool:
    return len(data) >= 12 and data[0:4] == _RIFF and data[8:12] == _WAVE


def wav_duration_sec(data: bytes) -> float | None:
    """从 WAV 头部解析时长（秒）。解析失败返回 None，绝不抛异常。"""

    try:
        if not is_wav(data):
            return None

        offset = 12
        byte_rate: int | None = None
        data_size: int | None = None
        total = len(data)

        while offset + 8 <= total:
            chunk_id = data[offset : offset + 4]
            chunk_size = struct.unpack_from("<I", data, offset + 4)[0]
            body = offset + 8

            if chunk_id == b"fmt " and body + 16 <= total:
                # fmt: audio_format(2) channels(2) sample_rate(4) byte_rate(4) ...
                byte_rate = struct.unpack_from("<I", data, body + 8)[0]
            elif chunk_id == b"data":
                available = max(0, total - body)
                data_size = min(chunk_size, available)
                break

            offset = body + chunk_size + (chunk_size % 2)

        if byte_rate and data_size is not None and byte_rate > 0:
            return round(data_size / byte_rate, 3)
        return None
    except Exception:  # noqa: BLE001 - 解析失败不影响主流程
        return None
