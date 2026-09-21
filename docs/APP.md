# 手机 App（Flutter / Android）

> 录音 → 分段实时转写 → 一键生成会议纪要 → 导出 Word。
> 音频只用于转写，**服务端不保存任何文件**；录音、转写、纪要全部留在手机本地。

## 1. 架构（分层与后端一致，方便后期维护）

```
app/lib/
├── main.dart                    # 依赖装配（Provider）
├── app.dart                     # MaterialApp / 主题
├── core/
│   ├── app_config.dart          # 默认地址、分段策略、采样率等常量
│   └── errors.dart              # AppException（UI 只处理这一种异常）
├── data/
│   ├── models/                  # AppSettings / Meeting / AudioSegment / MinutesDoc
│   ├── local/                   # sqflite（AppDatabase / MeetingDao）、SharedPreferences
│   └── api/meeting_api.dart     # 后端接口客户端（唯一的网络出口）
├── domain/
│   ├── audio/wav.dart           # WAV 读写/切分（纯 Dart，可单测）
│   ├── recording/
│   │   ├── segment_recorder.dart# 连续录音 + 静音自动切段（RMS 自适应阈值）
│   │   ├── upload_queue.dart    # 上传队列：并发 2、失败重试、状态回写
│   │   ├── rate_limiter.dart    # 本地令牌桶（9 请求/分钟，规避上游 429）
│   │   └── recording_foreground_service.dart # 前台服务（锁屏录音 + 通知栏）
│   ├── minutes/minutes_service.dart   # 生成纪要 + 导出 Word
│   ├── import/audio_importer.dart     # 导入已有 WAV（自动切块）
│   └── storage/app_paths.dart   # 本地文件布局
├── state/
│   ├── app_state.dart           # 设置 + 会议列表（ChangeNotifier）
│   ├── services.dart            # 应用级服务容器（跨页面共享）
│   └── recording_controller.dart# 单次录音会话状态机
└── ui/
    ├── format.dart
    └── screens/                 # home / recording / meeting_detail / settings
```

**解耦点**

| 边界 | 契约 | 换实现时只改 |
|---|---|---|
| 上游模型 | OpenAI 兼容 HTTP | 后端 `MMA_UPSTREAM_*`（App 不关心） |
| 后端 | `/health`、`/v1/segments/transcribe`、`/v1/minutes`、`/v1/export/docx`、`/v1/upstream/models` | `data/api/meeting_api.dart` |
| 存储 | `MeetingDao` | `data/local/*` |
| 录音 | `SegmentRecorder` 暴露 `Stream<RecordedSegment>` | `domain/recording/segment_recorder.dart` |
| 播放 | `just_audio` 的 `AudioPlayer`（详情页内使用） | `ui/screens/meeting_detail_screen.dart` |
| 限流 | `RateLimiter`（本地令牌桶） | `domain/recording/rate_limiter.dart` |
| 切段策略 | `AppConfig` 里的 4 个常量 | `core/app_config.dart` |

## 2. 录音与转写流水线

1. `record` 包以 **16 kHz / 单声道 / PCM16** 连续采集（`startStream`），全程不断流；
2. `SegmentRecorder` 每 100 ms 计算一次 RMS：
   - 自适应噪声地板（安静时缓慢跟随环境噪声），阈值 = max(220, 噪声×2.5)；
   - **静音 ≥ 600 ms 且本段 ≥ 8 s** → 切段；**段长到 45 s** → 强制切段；
3. 每段立刻打包成 WAV 写入 `AppDocuments/meetings/<会议 id>/seg_0001.wav`，入库（`pending`）；
4. `UploadQueue` 立刻上传（最多 2 路并发 + **本地令牌桶限流 9 请求/分钟**）：
   段 i 上传时，段 i+1 继续录。命中上游 429 时按 15s/30s/45s 退避重试，不直接判失败；
5. 成功后写回转写文本（`done`）；失败自动重试 3 次（401/400 这类确定性错误不重试），
   之后标记 `failed`，可在详情页一键重试（音频一直都在本机）。

### 锁屏/后台录音（前台服务）

- 录音开始时启动 Android 前台服务（`flutter_foreground_task`，`foregroundServiceType=microphone`），
  通知栏显示「正在录音」+「已转写 N 段 · 时长」，并提供 **停止录音** 按钮；
- 通知按钮 → 服务 isolate → `sendDataToMain` → 主 isolate 停止录音并跳转会议详情；
- 屏幕常亮改为可选项（设置页，默认关）：前台服务已保证锁屏继续录；
- App 从最近任务划掉时录音会停止（`stopWithTask: true`，不做"杀死也继续录"的承诺）。

> 会议结束后若还有分段在排队，队列会继续在后台跑；重进会议页会自动续传未完成的分段。

## 3. 纪要生成与导出

- 「生成纪要…」先弹层收集 **会议标题 / 日期 / 参会人 / 附加要求**（都可留空，会持久化到会议记录，
  下次自动带出），再把这些信息一并发给后端；后端让 LLM 输出**结构化 JSON**
  （议题/决议/待办/风险/下一步），解析失败自动修复重试；
- App 只把结构化 JSON 存本地，用 Flutter 组件渲染（不解析 Markdown，避免额外依赖）；
- 「导出 Word」调后端 `/v1/export/docx` 拿字节流，写到 `AppDocuments/exports/*.docx` 后
  交给系统分享（微信/邮件/文件管理器），可选是否附转写全文。

## 3.1 转写编辑

- 「转写」tab 每段都有编辑按钮，可修正人名/术语；修改写回本地库并更新会议时间戳；
- 若该会议已生成纪要，会提示「转写内容已修改，建议重新生成纪要」，一键重新生成；
- 音频不会因为编辑而改变（每段音频与分段一一对应）。

## 3.2 声音试听

- 每段右侧播放按钮：试听该段（`just_audio`，直接播放本地 WAV）；
- 一段播完自动接下一段，实现整场会议连播；播放中的分段在列表里高亮；
- 播放条显示「正在播放第 N 段 + 位置」，可暂停/停止；离开页面自动停止并释放播放器。

## 4. 设置页

用户只需要填两项（其余都有默认值）：

| 字段 | 默认值 | 说明 |
|---|---|---|
| BNUAPI 地址 | `https://chatapi.bnu.edu.cn/v1` | 逐请求通过 `X-Upstream-Base-URL` 透传给后端 |
| API Key | 空 | `Authorization: Bearer <key>`，只存手机本地 |

「测试连接」= 调后端 `/v1/upstream/models`（用当前 Key 拉模型列表，不消耗模型额度），
并检查 ASR/纪要模型 id 是否在列表中。

高级设置里可改：后端服务地址（默认 `http://127.0.0.1:8000`，真机需改成你自己部署的后端地址）、ASR 模型 id、纪要模型 id、
以及「录音时保持屏幕常亮」（默认关，见下）。

## 5. 构建与安装

```bash
source ~/tools/env.sh        # JDK17 + Flutter + Android SDK（见 DEV_ENV.md）
cd app
flutter pub get
flutter analyze && flutter test         # 静态检查 + 单测
flutter build apk --release             # 产物：build/app/outputs/flutter-apk/app-release.apk
# 或一键：bash scripts/build_apk.sh        （analyze + test + build + 复制到 dist/ + 打印签名证书）
```

**默认后端地址**：打包时用 `--dart-define` 注入，真实地址不入库——
把 `scripts/local.env.example` 复制成 `scripts/local.env`（已 gitignore）并填 `MMA_BACKEND_BASE_URL`，
`build_apk.sh` 会自动读取；不配则默认 `http://127.0.0.1:8000`。

**签名**：本机已配好正式 release keystore（位置与备份要求见 `docs/DEV_ENV.md`），
`flutter build apk --release` 自动用正式签名；换签名后旧 APK 不能覆盖安装，需先卸载（数据清空）。

安装到手机：把 `dist/bnu-meeting-<版本>.apk` 传到手机（微信/网盘/USB），允许「安装未知来源应用」后安装。
手机需要能访问后端地址（校园网内直连；校外可自行配置 HTTPS 反向代理）。

## 6. 当前限制

**有意不做**（产品已确认）：说话人分离、iOS、逐字真流式、AAC 低流量模式（上游只收 WAV）、
热词/术语表、PDF 导出、会议搜索/云备份、崩溃上报。

**受平台/上游约束**：

- **从最近任务划掉 App** → 录音停止（前台服务 `stopWithTask: true`，不做「杀死也继续录」的承诺）；
- 只支持 WAV（上游 vLLM 版 Qwen3-ASR 对 mp3/m4a 会报 `Invalid or unsupported audio file`）；
- 上游 BNUAPI 限流 **10 请求/分钟/用户**：App 本地限到 9/分钟，超长音频导入只会变慢、不会失败；
- 转写质量取决于上游 ASR + 手机麦克风；人名/专有名词建议在「转写」tab 手工修正后再生成纪要。

**排期外**：正式签名 keystore（当前用 Android debug key，内测够用）；iOS 端；校外访问优化。
