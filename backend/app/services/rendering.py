"""纪要渲染：把结构化模型渲染成 Markdown（纯函数，便于单测）。

导出层（docx）与前端都复用同一份结构化数据，渲染逻辑互不影响。
"""

from __future__ import annotations

from app.schemas.minutes import Minutes


def render_markdown(minutes: Minutes) -> str:
    lines: list[str] = []

    lines.append(f"# {minutes.title}".rstrip())
    lines.append("")

    meta: list[str] = []
    if minutes.meeting_date:
        meta.append(f"**日期**：{minutes.meeting_date}")
    if minutes.participants:
        meta.append(f"**参会人**：{'、'.join(minutes.participants)}")
    if meta:
        lines.extend(meta)
        lines.append("")

    lines.append("## 一、会议摘要")
    lines.append("")
    lines.append(minutes.overview.strip() or "（无）")
    lines.append("")

    lines.append("## 二、议题与讨论")
    lines.append("")
    if minutes.topics:
        for index, topic in enumerate(minutes.topics, start=1):
            lines.append(f"### {index}. {topic.title}")
            lines.append("")
            for point in topic.points:
                lines.append(f"- {point}")
            lines.append("")
    else:
        lines.append("（无）")
        lines.append("")

    lines.append("## 三、决议事项")
    lines.append("")
    if minutes.decisions:
        for index, decision in enumerate(minutes.decisions, start=1):
            lines.append(f"{index}. {decision}")
    else:
        lines.append("（无）")
    lines.append("")

    lines.append("## 四、待办事项")
    lines.append("")
    if minutes.action_items:
        lines.append("| 序号 | 待办事项 | 责任人 | 期限 |")
        lines.append("| --- | --- | --- | --- |")
        for index, item in enumerate(minutes.action_items, start=1):
            owner = item.owner or "—"
            due = item.due or "—"
            task = item.task.replace("|", "\\|")
            lines.append(f"| {index} | {task} | {owner} | {due} |")
    else:
        lines.append("（无）")
    lines.append("")

    if minutes.risks:
        lines.append("## 五、风险与遗留问题")
        lines.append("")
        for risk in minutes.risks:
            lines.append(f"- {risk}")
        lines.append("")

    if minutes.next_steps:
        lines.append("## 六、下一步计划")
        lines.append("")
        for step in minutes.next_steps:
            lines.append(f"- {step}")
        lines.append("")

    return "\n".join(lines).strip() + "\n"
