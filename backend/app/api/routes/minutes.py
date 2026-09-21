"""会议纪要接口。"""

from __future__ import annotations

from fastapi import APIRouter, Depends, Response

from app.api.deps import get_api_key, get_minutes_service, get_upstream_base_url
from app.api.http_utils import content_disposition, sanitize_filename
from app.schemas.common import UsageInfo
from app.schemas.minutes import MinutesRequest, MinutesResponse
from app.services.export import build_docx
from app.services.minutes import MinutesOutcome, MinutesService

router = APIRouter(prefix="/v1/minutes", tags=["minutes"])

_DOCX_MEDIA_TYPE = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"


def _to_usage(usage: dict) -> UsageInfo:
    return UsageInfo(
        prompt_tokens=usage.get("prompt_tokens"),
        completion_tokens=usage.get("completion_tokens"),
        total_tokens=usage.get("total_tokens"),
    )


def _docx_response(outcome: MinutesOutcome) -> Response:
    data = build_docx(outcome.minutes)
    filename = f"{sanitize_filename(outcome.minutes.title, fallback='会议纪要')}_纪要.docx"
    return Response(
        content=data,
        media_type=_DOCX_MEDIA_TYPE,
        headers={"Content-Disposition": content_disposition(filename)},
    )


@router.post("", response_model=MinutesResponse, summary="生成结构化会议纪要")
async def create_minutes(
    payload: MinutesRequest,
    api_key: str = Depends(get_api_key),
    service: MinutesService = Depends(get_minutes_service),
    base_url: str | None = Depends(get_upstream_base_url),
) -> MinutesResponse:
    outcome = await service.generate(payload, api_key, base_url=base_url)
    return MinutesResponse(
        minutes=outcome.minutes,
        markdown=outcome.markdown,
        model=outcome.model,
        usage=_to_usage(outcome.usage) if outcome.usage else None,
    )


@router.post(
    "/docx",
    summary="一步生成 Word 会议纪要",
    response_class=Response,
    responses={200: {"content": {_DOCX_MEDIA_TYPE: {}}}},
)
async def create_minutes_docx(
    payload: MinutesRequest,
    api_key: str = Depends(get_api_key),
    service: MinutesService = Depends(get_minutes_service),
    base_url: str | None = Depends(get_upstream_base_url),
) -> Response:
    outcome = await service.generate(payload, api_key, base_url=base_url)
    return _docx_response(outcome)
