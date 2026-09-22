# 会议模块（P5）

录音、转写、纪要全部在客户端完成，云端只提供模型能力（ASR + 对话模型）。
音频与文本都存在本机，不上传任何自建服务。

## 数据在哪里

| 内容 | 位置 |
| --- | --- |
| 录音分段（WAV） | `<应用数据目录>/meetings/<会议ID>/seg_0001.wav` |
| 会议、分段、转写、纪要（结构化） | `<应用数据目录>/index.sqlite`（表 `meetings` / `meeting_segments`） |
| 纪要 Markdown | 笔记库里的 `会议纪要/<标题>-<日期>.md`（自动写入并建索引，可在笔记页打开） |

应用数据目录（桌面端）：

- Windows：`%APPDATA%\com.bnu.notes\`
- Linux/macOS：`~/.local/share/com.bnu.notes/`（macOS 为 `~/Library/Application Support/com.bnu.notes/`）
- Android：应用私有目录（UI 上「音频目录」按钮可看到具体路径）

## 录音

- 采集：Web Audio `getUserMedia` → `ScriptProcessorNode` 取 Float32 → 转 PCM16 单声道。
  不使用 `MediaRecorder`：上游 ASR 只吃 WAV，而 WebView 的 `MediaRecorder` 在 WebKitGTK
  上编码 webm/opus 不稳定。
- 采样率：优先请求 16 kHz；设备不支持时按设备采样率（常见 48 kHz）采集，
  **按原采样率保存**（保真回放），送 ASR 前再重采样到 16 kHz。
- 落盘：每 1 秒把 PCM 追加写入当前分段文件，Rust 侧在开始分段时就写好 WAV 头，
  关闭分段时回写长度 → 意外退出最多丢最后一段的头部信息。
- 分段：默认 4 分钟一段（`src/stores/meetings.ts` 里的 `SEGMENT_MAX_MS`），
  切段时不中断录音（关闭上一段 → 立即开新段）。
- Android v1：只做前台录音；录音期间申请 `screen` Wake Lock 保持屏幕常亮（失败不阻断录音）。

### 平台麦克风权限

| 平台 | 需要什么 |
|---|---|
| Windows | ① 应用内已注册 WebView2 `PermissionRequested`，只放行本应用页面（`http(s)://tauri.localhost` / 开发服务器）的麦克风/摄像头，见 `src-tauri/src/webview_permissions.rs`；② 系统「设置 → 隐私和安全性 → 麦克风」需允许桌面应用访问麦克风，否则 `getUserMedia` 直接 `NotAllowedError` |
| Android | 系统运行时权限弹窗（Manifest 已声明 `RECORD_AUDIO`），拒绝后需到系统设置里重开 |
| Linux (WebKitGTK) | 未处理 WebKitGTK 媒体权限（WSL 无麦克风，无法实测）；如需 Linux 原生录音需再补 `enable-media-stream` + `permission-request` 放行 |

> 前端「录音自检」面板会打印每一步的错误名（`NotAllowedError` / `NotFoundError` / `NotReadableError`），
> 权限问题一眼可辨。

## 转写（送 ASR 前会再切一次段）

分段文件不会整段丢给 ASR，而是先在 Rust 侧做**静音切段**，减少长音频带来的超时/显存压力：

| 参数 | 值 |
| --- | --- |
| 采样率 | 16 kHz 单声道 16 bit |
| 分析帧长 | 100 ms |
| 切段条件 | 静音 ≥ 600 ms 且本段 ≥ 8 s |
| 强制切段 | 45 s |
| 收尾 | 残余 < 1 s 丢弃，≥ 1 s 保留 |
| 静音判定 | 自适应噪声地板 `max(220, floor×2.5)` |

请求编排（移植自既有会议 App）：

- 串行发送 + 令牌桶限流 **9 请求/分钟**（上游 10/分钟，留 1 个余量给纪要生成）；
- 失败重试 3 次：`429` → 15 s×次数，其它网络错误 → 2 s×次数；
  确定性 4xx（非 429，例如音频格式不对）直接标记该段失败，不重试；
- 每段完成即写入数据库，UI 通过 `meeting-progress` 事件看到进度；
- 单段失败不影响其它段，重新点「重新转写未完成段」即可续跑；
- 时间戳：每段内按切点偏移，跨段按已有时长累加，格式 `[HH:MM:SS]`。

## 纪要

- 提示词在 `crates/bnu-core/src/minutes.rs`（`MINUTES_SYSTEM_PROMPT` 等常量），
  移植自既有会议的提示词，铁律是「只依据原文、不编造、owner/due 不确定就 null」。
- 优先带 `response_format: {"type":"json_object"}` 请求；上游返回 400 时自动降级重试一次
  （能力状态在同一次生成内记忆）。
- JSON 解析失败会自动「修复重试」一次（带上解析错误与上次输出）。
- 长会议（≥ 60 000 字）先按 20 000 字分片抽取（map），再合并成完整纪要（reduce）。
- 结构化字段与既有会议后端一致：
  `title / meeting_date / participants / overview / topics[] / decisions[] / action_items[] / risks[] / next_steps[]`。
- 渲染成 Markdown 时：待办事项渲染为表格，无内容是写「（无）」而不是编内容。

## 预览模式

浏览器里 `npm run dev` 打开（非 Tauri 壳）时，会议模块走内存实现：
不录真实麦克风、不调模型，转写/纪要是示例文本并明确标注「预览模式」。
真实录音与转写请在桌面客户端里验证。

## 录音自检（Android spike 用）

会议页右上角「录音自检」：

1. 「运行环境自检」：报告 WebView 是否支持 `getUserMedia`、Wake Lock、Android UA；
2. 「录 5 秒并回放」：只在内存里录，显示采样率/峰值，并生成可回放的 WAV —— 
   能录能放就说明前台录音链路（权限 → 采集 → PCM → WAV）在目标设备上可用。

## 接线（客户端命令）

| 命令 | 作用 |
| --- | --- |
| `meeting_create` / `meeting_list` / `meeting_detail` / `meeting_rename` | 会议 CRUD |
| `meeting_delete`（`deleteFiles`） | 删除记录；勾选时同时删除音频目录（UI 二次确认） |
| `meeting_start_segment` / `meeting_append_pcm` / `meeting_close_segment` | 分段写入（PCM 走 base64，避免大数组 JSON） |
| `meeting_transcribe` / `meeting_cancel_transcribe` | 转写（异步，事件 `meeting-progress` / `meeting-transcribed`） |
| `meeting_generate_minutes` | 生成纪要并写入笔记库 |
| `meeting_dir` | 返回音频目录（播放走 `asset://` 协议，配置见 `tauri.conf.json`） |

## 已知限制

- 无实时流式转写、无说话人分离（产品范围外）；
- 录音期间不允许切换会议（UI 会拦截）；
- 纪要生成的 completion token 上限 8192；转写文本上限 150 万字（超出直接报错，不静默截断）；
- Android 音频焦点/中断（来电、其它 App 抢占麦克风）v1 不做处理，息屏或切后台可能中断录音；
- Linux 桌面端未处理 WebKitGTK 媒体权限，录音仅在 Windows / Android 验证。
