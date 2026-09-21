"""导出接口：对（可能已被用户编辑的）结构化纪要重新渲染 Word。"""

from __future__ import annotations

from fastapi import APIRouter, Depends, Response

from app.api.deps import get_api_key
from app.api.http_utils import content_disposition, sanitize_filename
from app.schemas.minutes import DocxExportRequest
from app.services.export import build_docx

router = APIRouter(prefix="/v1/export", tags=["export"])

_DOCX_MEDIA_TYPE = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"


@router.post(
    "/docx",
    summary="按结构化纪要导出 Word",
    response_class=Response,
    responses={200: {"content": {_DOCX_MEDIA_TYPE: {}}}},
)
async def export_docx(
    payload: DocxExportRequest,
    _api_key: str = Depends(get_api_key),
) -> Response:
    data = build_docx(
        payload.minutes,
        include_transcript=payload.include_transcript,
        transcript=payload.transcript,
    )
    filename = f"{sanitize_filename(payload.minutes.title, fallback='会议纪要')}_纪要.docx"
    return Response(
        content=data,
        media_type=_DOCX_MEDIA_TYPE,
        headers={"Content-Disposition": content_disposition(filename)},
    )
