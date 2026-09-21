"""分段转写相关的请求/响应模型。"""

from __future__ import annotations

from pydantic import BaseModel, Field


class TranscriptionResponse(BaseModel):
    """单个音频分段的转写结果。

    服务端无状态：``session_id``/``seq`` 原样回传，仅用于 App 侧拼接排序。
    """

    session_id: str | None = Field(default=None, description="App 侧会话 id，原样回传")
    seq: int | None = Field(default=None, description="分段序号，原样回传")
    text: str = Field(description="该段转写文本")
    duration_sec: float | None = Field(default=None, description="服务端解析出的音频时长（秒）")
    model: str = Field(description="实际使用的 ASR 模型")
