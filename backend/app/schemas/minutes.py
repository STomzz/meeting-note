"""会议纪要相关的请求/响应模型。

结构化纪要模型（:class:`Minutes`）是 LLM 输出与 Word 导出之间的稳定契约：
- LLM 输出必须是其合法 JSON；解析失败会触发一次修复重试。
- 前端/导出层只依赖该模型，不关心 LLM 的输出格式细节。
"""

from __future__ import annotations

from pydantic import BaseModel, Field

from app.schemas.common import UsageInfo


class ActionItem(BaseModel):
    """待办事项。"""

    task: str = Field(description="待办内容")
    owner: str | None = Field(default=None, description="责任人（原文未提及则为空）")
    due: str | None = Field(default=None, description="期限（原文未提及则为空）")
    source: str | None = Field(default=None, description="原文依据（可选，防幻觉用）")


class Topic(BaseModel):
    """议题与讨论要点。"""

    title: str = Field(description="议题名称")
    points: list[str] = Field(default_factory=list, description="讨论要点")


class Minutes(BaseModel):
    """结构化会议纪要（稳定契约）。"""

    title: str = Field(description="会议标题")
    meeting_date: str | None = Field(
        default=None, description="会议日期（原文/用户提供，未知则空）"
    )
    participants: list[str] = Field(default_factory=list, description="参会人（未知则空）")
    overview: str = Field(description="整体摘要（3~6 句）")
    topics: list[Topic] = Field(default_factory=list, description="议题与讨论")
    decisions: list[str] = Field(default_factory=list, description="达成的决议/结论")
    action_items: list[ActionItem] = Field(default_factory=list, description="待办事项")
    risks: list[str] = Field(default_factory=list, description="风险、遗留问题")
    next_steps: list[str] = Field(default_factory=list, description="下一步计划")


class MinutesRequest(BaseModel):
    """纪要生成请求。"""

    transcript: str = Field(description="会议完整转写文本")
    title: str | None = Field(default=None, description="会议标题（可选）")
    meeting_date: str | None = Field(default=None, description="会议日期（可选）")
    participants: list[str] | None = Field(default=None, description="参会人（可选）")
    template: str | None = Field(
        default=None, description="附加要求/模板说明（可选，例如'重点提炼风险'）"
    )
    language: str = Field(default="zh", description="输出语言，默认中文")
    model: str | None = Field(default=None, description="LLM 模型 id，默认取服务端配置")


class MinutesResponse(BaseModel):
    minutes: Minutes
    markdown: str = Field(description="已渲染好的 Markdown 纪要")
    model: str
    usage: UsageInfo | None = None


class DocxExportRequest(BaseModel):
    """Word 导出请求（可用于对已编辑的纪要重新导出）。"""

    minutes: Minutes
    transcript: str | None = Field(default=None, description="可选：附在文末的转写全文")
    include_transcript: bool = Field(default=False, description="是否把转写全文附在文末")
