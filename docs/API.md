# 接口文档（v1）

Base URL：`http://YOUR_SERVER_IP:8000`（校园网/内网，按你的部署替换）；交互式文档：`/docs`。

统一鉴权：所有业务接口要求 `Authorization: Bearer <BNUAPI key>`。
（服务端若配置了 `MMA_ALLOW_SERVER_KEY=true` 且有 `MMA_UPSTREAM_API_KEY`，缺省时用兜底 key，仅开发用。）

上游地址（App 设置页的 "BNUAPI 地址"）默认取服务端配置 `MMA_UPSTREAM_BASE_URL`，
客户端可用请求头 `X-Upstream-Base-URL` 逐请求覆盖（如 `https://chatapi.bnu.edu.cn/v1`，
主机需在白名单 `MMA_UPSTREAM_ALLOWED_HOSTS` 内）。

错误统一结构：

```json
{"error": {"code": "upstream_error", "message": "上游返回 400（audio/transcriptions）：...", "request_id": "abc123", "details": null}}
```

---

## GET /health

```json
{"status": "ok", "app": "bnu-meeting-api", "version": "0.1.0",
 "provider": "openai_compatible", "upstream_base_url": "http://127.0.0.1:3000/v1"}
```

## GET /v1/defaults

```json
{"asr": "qwen3-asr-1.7b", "minutes": "Qwen-Inno-35B-v1"}
```

## GET /v1/upstream/models

用请求头里的 Key 拉取上游可用模型（App 设置页「测试连接」用，不消耗模型额度）。

```bash
curl -H "Authorization: Bearer sk-xxx" \
     -H "X-Upstream-Base-URL: https://chatapi.bnu.edu.cn/v1" \
     http://YOUR_SERVER_IP:8000/v1/upstream/models
```

```json
{"models": ["qwen3-asr-1.7b", "qwen3.8-27b", "..."]}
```

---

## POST /v1/segments/transcribe

`multipart/form-data`，服务端无状态，转完即弃。

| 字段 | 必填 | 说明 |
|---|---|---|
| `file` | 是 | WAV 音频分段（16kHz 单声道最佳；上限 `MMA_MAX_AUDIO_MB`，默认 50MB） |
| `session_id` | 否 | App 侧会话 id，原样回传 |
| `seq` | 否 | 分段序号，原样回传 |
| `model` | 否 | ASR 模型 id，默认服务端配置（`qwen3-asr-1.7b`） |
| `language` | 否 | 语言提示，如 `zh` |
| `prompt` | 否 | 热词/上下文提示（逗号分隔人名术语） |

响应：

```json
{"session_id": "sess-1", "seq": 3, "text": "欢迎参加今天的会议。", "duration_sec": 12.5, "model": "qwen3-asr-1.7b"}
```

```bash
curl -X POST http://YOUR_SERVER_IP:8000/v1/segments/transcribe \
  -H "Authorization: Bearer sk-xxx" \
  -F file=@seg_003.wav -F session_id=s1 -F seq=3
```

---

## POST /v1/minutes

`application/json`：

```json
{
  "transcript": "张老师：…… 李工：……",
  "title": "教务系统升级项目周会",
  "meeting_date": "2026-09-21",
  "participants": ["张老师", "李工"],
  "template": "重点提炼风险与回退方案",
  "model": "Qwen-Inno-35B-v1"
}
```

`transcript` 必填；其余可选。响应：

```json
{
  "minutes": {
    "title": "教务系统升级项目周会",
    "meeting_date": "2026-09-21",
    "participants": ["张老师", "李工"],
    "overview": "……",
    "topics": [{"title": "数据迁移", "points": ["……"]}],
    "decisions": ["……"],
    "action_items": [{"task": "导出不一致清单", "owner": "李工", "due": "10月10日前", "source": "……"}],
    "risks": ["……"],
    "next_steps": ["……"]
  },
  "markdown": "# 教务系统升级项目周会\n\n## 一、会议摘要…",
  "model": "Qwen-Inno-35B-v1",
  "usage": {"prompt_tokens": 3120, "completion_tokens": 890, "total_tokens": 4010}
}
```

## POST /v1/minutes/docx

请求体同 `/v1/minutes`，直接返回 `.docx`（`Content-Disposition: attachment; filename*=UTF-8''…`）。

## POST /v1/export/docx

对已编辑的纪要重新导出：

```json
{"minutes": {"...": "结构同响应中的 minutes"}, "include_transcript": true, "transcript": "全文…"}
```

---

## 行为说明

- **上游地址可由客户端指定**：请求头 `X-Upstream-Base-URL: https://chatapi.bnu.edu.cn/v1`
  （App 设置页的"BNUAPI 地址"）。不传则用服务端配置 `MMA_UPSTREAM_BASE_URL`；
  主机需在 `MMA_UPSTREAM_ALLOWED_HOSTS` 白名单内（默认 `chatapi.bnu.edu.cn`，置空=不限制）。
- **长会议**：`transcript` ≥ 60,000 字自动走 map-reduce（分段抽取 + 合并），接口与响应不变。
- **纪要 JSON 解析失败**：服务端自动让模型修复一次，仍失败返回 `upstream_invalid_response`。
- **上游不支持 `response_format`**：自动降级为普通模式，仅打日志，不影响客户端。
- **音频格式**：当前仅支持 WAV（mp3/m4a 上游不支持）；如需接收压缩音频，开启 `MMA_TRANSCODE_ENABLED=true` 并安装 ffmpeg。
