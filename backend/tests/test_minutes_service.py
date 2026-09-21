"""纪要服务单测：JSON 提取、切分、生成与修复。"""

from __future__ import annotations

import json

from app.schemas.minutes import MinutesRequest
from app.services.minutes import MinutesService, extract_json_object, split_transcript
from tests.conftest import DEFAULT_MINUTES

VALID_JSON = json.dumps(DEFAULT_MINUTES, ensure_ascii=False)


def make_service(fake_provider, settings) -> MinutesService:
    from app.core.prompts import create_prompts_env

    return MinutesService(fake_provider, settings, create_prompts_env(settings.prompts_dir))


def test_extract_json_object_plain():
    assert extract_json_object(VALID_JSON)["title"] == "产品周会"


def test_extract_json_object_with_code_fence_and_noise():
    text = f"好的，以下是纪要：\n```json\n{VALID_JSON}\n```\n希望有帮助。"
    assert extract_json_object(text)["title"] == "产品周会"


def test_extract_json_object_invalid():
    assert extract_json_object("完全不是 JSON") is None
    assert extract_json_object("") is None


def test_split_transcript_keeps_paragraphs():
    transcript = "\n".join(f"第{i}行内容" for i in range(10))
    chunks = split_transcript(transcript, chunk_chars=20)
    assert len(chunks) > 1
    assert "".join(chunks).replace("\n", "") == transcript.replace("\n", "")


async def test_generate_single_call(fake_provider, settings):
    service = make_service(fake_provider, settings)
    outcome = await service.generate(
        MinutesRequest(transcript="张三：我们来讨论发布计划。", title="产品周会"),
        api_key="user-key",
    )

    assert outcome.minutes.title == "产品周会"
    assert len(fake_provider.chat_calls) == 1
    assert "## 一、会议摘要" in outcome.markdown
    assert "待办事项" in outcome.markdown
    assert "准备发布公告" in outcome.markdown


async def test_generate_repairs_invalid_json(fake_provider, settings):
    fake_provider.chat_responses.extend(["这不是 JSON", VALID_JSON])
    service = make_service(fake_provider, settings)

    outcome = await service.generate(MinutesRequest(transcript="测试内容"), api_key="user-key")

    assert outcome.minutes.title == "产品周会"
    assert len(fake_provider.chat_calls) == 2  # 原始调用 + 修复调用


async def test_generate_map_reduce_for_long_transcript(fake_provider, settings):
    long_transcript = "\n".join(f"第{i}段：讨论了一些内容。" for i in range(50))
    short_settings = settings.model_copy(
        update={"minutes_map_reduce_threshold_chars": 100, "minutes_chunk_chars": 50}
    )
    service = make_service(fake_provider, short_settings)

    outcome = await service.generate(MinutesRequest(transcript=long_transcript), api_key="user-key")

    assert outcome.minutes.overview
    # 分段抽取（>1 次）+ 合并（1 次）
    assert len(fake_provider.chat_calls) > 2


async def test_empty_transcript_rejected(fake_provider, settings):
    import pytest

    from app.core.errors import BadRequestError

    service = make_service(fake_provider, settings)
    with pytest.raises(BadRequestError):
        await service.generate(MinutesRequest(transcript="   "), api_key=None)
