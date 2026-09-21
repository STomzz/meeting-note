# 架构说明

## 1. 分层与依赖方向

```
api/（路由，薄） ──► services/（业务，纯） ──► providers/（上游，可替换）
      │                    │
      │                    └──► schemas/（契约）
      └──► core/（配置/错误/日志/提示词）
```

- **依赖只向下**：services 不 import api；providers 不 import services；
  schemas 不依赖任何层。禁止反向依赖，保证可测与可替换。
- **api 层只做三件事**：解析参数、取依赖、调用 service 并转成响应。
- **services 层不碰 HTTP**：入参是普通数据结构，出参是 schema 模型，纯函数/纯类，可单测。
- **providers 层只碰 HTTP**：把 httpx 异常归一化成 `UpstreamError`，不掺杂业务判断。

## 2. 关键解耦点（后期维护/换上游的主要入口）

| 关注点 | 位置 | 替换方式 |
|---|---|---|
| 上游模型服务 | `app/providers/` | 实现 `UpstreamProvider` 协议 → 在 `factory._REGISTRY` 注册 → 改 `MMA_UPSTREAM_PROVIDER` |
| 提示词/纪要风格 | `app/prompts/*.md(.j2)` | 直接改文件，或用镜像挂载覆盖，无需改代码 |
| 模型 id / 温度 / 长度 | `app/core/config.py` + 环境变量 | 改环境变量即可；App 也可逐请求覆盖模型 |
| 纪要结构（字段） | `app/schemas/minutes.py` | 改 `Minutes` 模型 + 提示词中的 JSON 结构说明 |
| 渲染格式 | `app/services/rendering.py`（Markdown）、`app/services/export.py`（Word） | 新增导出器即可，不影响生成逻辑 |
| 音频格式支持 | `app/services/audio.py` + `MMA_TRANSCODE_ENABLED` | 打开转码（需 ffmpeg）即可接收 AAC/MP3 |

## 3. 数据流（一次会议）

```
App 录音线程                    服务端（无状态）                    上游
─────────────                  ────────────────                  ─────
段1 录完 ──POST /v1/segments/transcribe──► 校验 WAV ──► /audio/transcriptions ──► Qwen3-ASR(172)
   │（同时继续录段2）              │ 内存转发，不落盘
   ◄──────────── {seq, text} ─────┘
…
会议结束 ──POST /v1/minutes────────► 渲染提示词 ──► /chat/completions ──► LLM(BNUAPI)
   ◄──── {minutes(JSON), markdown} ─┘
   ──POST /v1/minutes/docx─────────► python-docx 渲染 ──► .docx 字节流
```

- 服务端**不写文件、不建库、不存会话**；`session_id`/`seq` 仅原样回传，用于 App 侧排序拼接。
- 长会议（转写 ≥ `MMA_MINUTES_MAP_REDUCE_THRESHOLD_CHARS`）：先分段抽取（map，并发 2），再合并（reduce）。

## 4. 已固化的设计决策

| 决策 | 原因 |
|---|---|
| 只收 WAV | 实测当前 vLLM 版 Qwen3-ASR 不支持 mp3/m4a（`Invalid or unsupported audio file`） |
| 时长以本地 WAV 解析为准 | 上游 `usage.seconds` 是整数秒，本地解析更准且不依赖上游 |
| 纪要 = 结构化 JSON + 确定性渲染 | LLM 只负责"抽取信息"，Markdown/Word 由代码渲染，格式稳定、可编辑、可回滚 |
| 解析失败自动修复重试 1 次 | 小模型偶发输出不合格 JSON，避免把失败透传给用户 |
| `response_format=json_object` 可降级 | 部分上游不支持该参数，遇到 400 自动去掉并记住 |
| 服务端不存 key | App 传用户自己的 BNUAPI key，限流/计费自然归到个人 |
| 上游地址可由客户端逐请求覆盖（`X-Upstream-Base-URL`） | 用户在 App 里填的就是 BNUAPI 地址；服务端只做主机白名单校验，避免变成任意代理（SSRF） |
| App 端录音切段而不是服务端切段 | 服务端零存储、零会话状态；切段策略（8s/600ms/45s）全部在端侧，参数集中在 `AppConfig` |

> App 侧的分层、录音流水线与构建方式见 [APP.md](APP.md)。

## 5. 可观测性

- 每个请求带 `X-Request-ID`（可由客户端传入，服务端生成兜底），错误响应含该 id；
- 上游重试会打 warning 日志（含重试次数与原因）；
- 未捕获异常记录完整堆栈。
