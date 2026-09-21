"""会议纪要生成服务。

流程：
- 短会议（转写 < 阈值）：一次 LLM 调用直接产出结构化纪要；
- 长会议：先按片段并行抽取（map），再合并（reduce），避免超出上下文并减少遗漏；
- 输出统一解析为 :class:`~app.schemas.minutes.Minutes`，解析失败自动"修复重试"一次。

所有提示词都在 ``app/prompts/`` 下外置，修改提示词无需改代码。
"""

from __future__ import annotations

import asyncio
import json
import logging
import re
from dataclasses import dataclass, field

import jinja2
from pydantic import ValidationError

from app.core.config import Settings
from app.core.errors import BadRequestError, UpstreamError, UpstreamInvalidResponseError
from app.providers.base import ChatProvider
from app.schemas.minutes import Minutes, MinutesRequest
from app.services.rendering import render_markdown

logger = logging.getLogger(__name__)

_MAX_CONCURRENT_CHUNKS = 2
_CODE_FENCE_RE = re.compile(r"^```[a-zA-Z0-9_-]*\s*|\s*```$")


@dataclass
class MinutesOutcome:
    minutes: Minutes
    markdown: str
    model: str
    usage: dict = field(default_factory=dict)


def extract_json_object(text: str) -> dict | None:
    """从 LLM 输出中提取第一个合法 JSON 对象（容忍代码块围栏与前后废话）。"""

    if not text:
        return None
    cleaned = _CODE_FENCE_RE.sub("", text.strip())
    decoder = json.JSONDecoder()
    for index, char in enumerate(cleaned):
        if char != "{":
            continue
        try:
            obj, _ = decoder.raw_decode(cleaned[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            return obj
    return None


def split_transcript(transcript: str, chunk_chars: int) -> list[str]:
    """按段落切分转写文本，尽量保持段落完整。"""

    if len(transcript) <= chunk_chars:
        return [transcript]

    chunks: list[str] = []
    current: list[str] = []
    current_len = 0

    for line in transcript.splitlines():
        line_len = len(line) + 1
        if current and current_len + line_len > chunk_chars:
            chunks.append("\n".join(current).strip())
            current, current_len = [], 0
        current.append(line)
        current_len += line_len

    if current:
        chunks.append("\n".join(current).strip())

    return [chunk for chunk in chunks if chunk]


class MinutesService:
    """纪要生成服务（与具体上游解耦，只依赖 ChatProvider）。"""

    def __init__(
        self,
        provider: ChatProvider,
        settings: Settings,
        prompts: jinja2.Environment,
    ) -> None:
        self._provider = provider
        self._settings = settings
        self._prompts = prompts
        self._json_format_supported: bool | None = None

    # ------------------------------------------------------------------ 对外
    async def generate(
        self, request: MinutesRequest, api_key: str | None, base_url: str | None = None
    ) -> MinutesOutcome:
        transcript = (request.transcript or "").strip()
        if not transcript:
            raise BadRequestError("转写文本为空，无法生成纪要")
        if len(transcript) > self._settings.max_transcript_chars:
            raise BadRequestError(
                f"转写文本过长（{len(transcript)} 字），"
                f"上限 {self._settings.max_transcript_chars} 字"
            )

        model = request.model or self._settings.default_minutes_model

        if len(transcript) >= self._settings.minutes_map_reduce_threshold_chars:
            logger.info("转写 %s 字，走 map-reduce 纪要流程", len(transcript))
            content, usage = await self._generate_map_reduce(
                request, transcript, model, api_key, base_url
            )
        else:
            content, usage = await self._generate_single(
                request, transcript, model, api_key, base_url
            )

        minutes = await self._parse_or_repair(content, model, api_key, base_url)
        return MinutesOutcome(
            minutes=minutes,
            markdown=render_markdown(minutes),
            model=model,
            usage=usage,
        )

    # ------------------------------------------------------------- 单次生成
    async def _generate_single(
        self,
        request: MinutesRequest,
        transcript: str,
        model: str,
        api_key: str | None,
        base_url: str | None = None,
    ) -> tuple[str, dict]:
        messages = [
            {"role": "system", "content": self._render("minutes_system.md", {})},
            {
                "role": "user",
                "content": self._render(
                    "minutes_user.md.j2",
                    {
                        "title": request.title,
                        "meeting_date": request.meeting_date,
                        "participants_text": "、".join(request.participants or []) or None,
                        "template": request.template,
                        "language": request.language,
                        "transcript": transcript,
                    },
                ),
            },
        ]
        result = await self._chat(messages, model, api_key, base_url=base_url)
        return result.content, result.usage

    # ------------------------------------------------------------- map-reduce
    async def _generate_map_reduce(
        self,
        request: MinutesRequest,
        transcript: str,
        model: str,
        api_key: str | None,
        base_url: str | None = None,
    ) -> tuple[str, dict]:
        chunks = split_transcript(transcript, self._settings.minutes_chunk_chars)
        semaphore = asyncio.Semaphore(_MAX_CONCURRENT_CHUNKS)

        async def extract(index: int, chunk: str) -> dict | None:
            async with semaphore:
                messages = [
                    {"role": "system", "content": self._render("chunk_system.md", {})},
                    {
                        "role": "user",
                        "content": self._render(
                            "chunk_user.md.j2",
                            {"index": index, "total": len(chunks), "chunk": chunk},
                        ),
                    },
                ]
                result = await self._chat(
                    messages, model, api_key, expect_json=True, base_url=base_url
                )
                return extract_json_object(result.content)

        partials = await asyncio.gather(
            *(extract(index, chunk) for index, chunk in enumerate(chunks, start=1))
        )
        valid_partials = [item for item in partials if item]
        if not valid_partials:
            raise UpstreamInvalidResponseError("长会议分段抽取全部失败，无法生成纪要")

        merge_input = json.dumps(valid_partials, ensure_ascii=False, indent=1)[
            : self._settings.max_transcript_chars
        ]
        messages = [
            {"role": "system", "content": self._render("merge_system.md", {})},
            {
                "role": "user",
                "content": self._render(
                    "minutes_user.md.j2",
                    {
                        "title": request.title,
                        "meeting_date": request.meeting_date,
                        "participants_text": "、".join(request.participants or []) or None,
                        "template": request.template,
                        "language": request.language,
                        "transcript": "以下是按时间顺序的分段抽取结果：\n" + merge_input,
                    },
                ),
            },
        ]
        result = await self._chat(messages, model, api_key, expect_json=True, base_url=base_url)
        return result.content, result.usage

    # ------------------------------------------------------------- 解析修复
    async def _parse_or_repair(
        self, content: str, model: str, api_key: str | None, base_url: str | None = None
    ) -> Minutes:
        minutes, error = self._try_parse(content)
        if minutes is not None:
            return minutes

        logger.warning("纪要 JSON 解析失败，触发修复重试：%s", error)
        repair_messages = [
            {"role": "system", "content": self._render("minutes_system.md", {})},
            {
                "role": "user",
                "content": self._render(
                    "repair_user.md.j2",
                    {"error": error, "previous_output": content[:6000]},
                ),
            },
        ]
        repaired = await self._chat(
            repair_messages, model, api_key, expect_json=True, base_url=base_url
        )
        minutes, second_error = self._try_parse(repaired.content)
        if minutes is not None:
            return minutes

        raise UpstreamInvalidResponseError(
            f"纪要 JSON 两次解析失败：{second_error}",
            details=repaired.content[:1000],
        )

    @staticmethod
    def _try_parse(content: str) -> tuple[Minutes | None, str | None]:
        payload = extract_json_object(content)
        if payload is None:
            return None, "输出中找不到合法 JSON 对象"
        try:
            return Minutes.model_validate(payload), None
        except ValidationError as exc:
            return None, str(exc)[:800]

    # ------------------------------------------------------------------ 调用
    async def _chat(
        self,
        messages: list[dict],
        model: str,
        api_key: str | None,
        *,
        expect_json: bool = True,
        base_url: str | None = None,
    ):
        response_format: dict | None = None
        if (
            expect_json
            and self._settings.use_json_response_format
            and self._json_format_supported is not False
        ):
            response_format = {"type": "json_object"}

        try:
            return await self._provider.complete(
                messages=messages,
                model=model,
                api_key=api_key,
                temperature=self._settings.minutes_temperature,
                max_tokens=self._settings.minutes_max_tokens,
                response_format=response_format,
                base_url=base_url,
            )
        except UpstreamError as exc:
            # 某些上游不支持 response_format：降级重试一次，并记住该能力
            if response_format is not None and exc.upstream_status == 400:
                logger.info("上游不支持 response_format=json_object，降级为普通模式")
                self._json_format_supported = False
                return await self._provider.complete(
                    messages=messages,
                    model=model,
                    api_key=api_key,
                    temperature=self._settings.minutes_temperature,
                    max_tokens=self._settings.minutes_max_tokens,
                    base_url=base_url,
                )
            raise

    def _render(self, template_name: str, context: dict) -> str:
        return self._prompts.get_template(template_name).render(**context)
