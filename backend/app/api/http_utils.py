"""HTTP 小工具。"""

from __future__ import annotations

import re
from urllib.parse import quote

_UNSAFE_FILENAME_RE = re.compile(r'[\\/:*?"<>|\r\n\t]+')


def sanitize_filename(name: str, *, fallback: str = "meeting") -> str:
    cleaned = _UNSAFE_FILENAME_RE.sub("_", (name or "").strip())
    cleaned = cleaned.strip("._ ") or fallback
    return cleaned[:80]


def content_disposition(filename: str) -> str:
    """生成兼容中文文件名的 Content-Disposition。"""

    return f"attachment; filename*=UTF-8''{quote(filename)}"
