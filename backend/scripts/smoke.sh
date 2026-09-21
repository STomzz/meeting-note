#!/usr/bin/env bash
# 端到端冒烟：健康检查 → 分段转写 → 纪要生成 → Word 导出
#
# 用法：
#   BASE_URL=http://127.0.0.1:8000 API_KEY=sk-xxx SAMPLE_WAV=/path/a.wav bash scripts/smoke.sh
#
# 说明：
#   - API_KEY 可省略（服务端 MMA_ALLOW_SERVER_KEY=true 时用兜底 key）
#   - SAMPLE_WAV 可省略（跳过转写步骤）
set -euo pipefail

cd "$(dirname "$0")/.."

BASE_URL="${BASE_URL:-http://127.0.0.1:8000}"
API_KEY="${API_KEY:-}"
SAMPLE_WAV="${SAMPLE_WAV:-}"
ASR_MODEL="${ASR_MODEL:-qwen3-asr-1.7b}"
MINUTES_MODEL="${MINUTES_MODEL:-Qwen-Inno-35B-v1}"
OUT_DIR="${OUT_DIR:-smoke-out}"

# 优先用项目 venv 里的 python（含 python-docx 等依赖）
PY_BIN="python3"
[[ -x .venv/bin/python ]] && PY_BIN=".venv/bin/python"

mkdir -p "$OUT_DIR"
UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-}"
AUTH=()
[[ -n "$API_KEY" ]] && AUTH=(-H "Authorization: Bearer $API_KEY")
[[ -n "$UPSTREAM_BASE_URL" ]] && AUTH+=(-H "X-Upstream-Base-URL: $UPSTREAM_BASE_URL")

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$1"; }

step "1/4 健康检查"
curl -sf "$BASE_URL/health" | "$PY_BIN" -m json.tool

if [[ -n "$SAMPLE_WAV" ]]; then
  step "2/4 分段转写：$SAMPLE_WAV"
  curl -sf "${AUTH[@]}" \
    -F "file=@${SAMPLE_WAV}" \
    -F "session_id=smoke" \
    -F "seq=1" \
    -F "model=${ASR_MODEL}" \
    "$BASE_URL/v1/segments/transcribe" | tee "$OUT_DIR/segment.json" | "$PY_BIN" -m json.tool
else
  step "2/4 跳过转写（未提供 SAMPLE_WAV）"
fi

step "3/4 生成纪要（含 Markdown）"
"$PY_BIN" - "$MINUTES_MODEL" > "$OUT_DIR/minutes_request.json" <<'PY'
import json, sys, pathlib
transcript = pathlib.Path("scripts/sample_transcript.txt").read_text(encoding="utf-8")
print(json.dumps({
    "transcript": transcript,
    "title": "教务系统升级项目周会",
    "meeting_date": "2026-09-21",
    "participants": ["张老师", "李工", "王工"],
    "model": sys.argv[1],
}, ensure_ascii=False))
PY

curl -sf "${AUTH[@]}" -H "Content-Type: application/json" \
  -d @"$OUT_DIR/minutes_request.json" \
  "$BASE_URL/v1/minutes" | tee "$OUT_DIR/minutes.json" | "$PY_BIN" -c '
import json, sys
data = json.load(sys.stdin)
print(data["markdown"])
print("--- 模型:", data["model"], "用量:", data.get("usage"))
'

step "4/4 导出 Word"
curl -sf "${AUTH[@]}" -H "Content-Type: application/json" \
  -d @"$OUT_DIR/minutes_request.json" \
  -o "$OUT_DIR/minutes.docx" \
  "$BASE_URL/v1/minutes/docx"

ls -lh "$OUT_DIR/minutes.docx"
"$PY_BIN" - "$OUT_DIR/minutes.docx" <<'PY'
import sys
from docx import Document
doc = Document(sys.argv[1])
print("段落数:", len(doc.paragraphs), "表格数:", len(doc.tables))
PY

printf '\n\033[1;32m冒烟通过 ✅  产物在 %s/\033[0m\n' "$OUT_DIR"
