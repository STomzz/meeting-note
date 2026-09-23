# 模型端点说明（客户端直连）

所有请求由客户端本地（Rust `reqwest`，Tauri 命令层）带着设置里的 baseURL + API Key 直连，不经过任何中间服务器。
浏览器预览模式（`npm run dev`）不发起真实请求。

## 四类能力

| 能力 | 接口 | 推荐模型 | 是否必需 | 未配置时的行为 |
|---|---|---|---|---|
| 对话 / 纪要 | `POST {base}/v1/chat/completions` | `Qwen-Inno-35B-v1` | 必需 | 问答、会议纪要不可用，其余功能正常 |
| 语音转写 | `POST {base}/v1/audio/transcriptions` | `qwen3-asr-1.7b` | 必需 | 会议只录音、不转写 |
| 嵌入 | `POST {base}/v1/embeddings` | `bge-m3`（1024 维） | 可选 | 检索降级为 FTS5 全文 + LIKE 兜底 |
| 重排 | `POST {base}/v1/rerank` | `bge-reranker-v2-m3` | 可选 | 跳过精排，用融合排序 |

baseURL 写 `https://host` 或 `https://host/v1` 都可以，客户端会自动补 `/v1`（已含则不重复追加）。

**集群网关默认地址（2026-09-23 起）**：`https://chatapi.bnu.edu.cn/bnuapi`
（门户融合后 BNUAPI 控制台挪到 `/bnuapi`，机器通道随之是 `/bnuapi/v1/*`；请求实际打到
`https://chatapi.bnu.edu.cn/bnuapi/v1/chat/completions` 等。同域旧路径 `/v1/*` 仍兼容保留，
已配置成 `https://chatapi.bnu.edu.cn` 的客户端不用改也能用。「填入推荐配置」已按新地址预填四项。）

## 推理模型注意事项

`Qwen-Inno-35B-v1` 默认先输出思考过程（响应里的 `reasoning` 字段），慢且费 token。请求带：

```json
{ "chat_template_kwargs": { "enable_thinking": false } }
```

实测从 2.3 s / 653 tokens 降到 0.4 s / 2 tokens，且不再有 `reasoning` 字段。
设置页「填入推荐配置」已预置该参数（对话能力的"附加参数"）。

客户端容错逻辑（`crates/bnu-core/src/models.rs`）：

- `content` 为空但有 `reasoning` → 回退返回思考文本；
- `finish_reason=length` 且无 content → 明确报错，提示提高 `max_tokens` 或关闭思考。

## 实测记录（2026-09-23，新前缀 `/bnuapi`）

用仓库里的真实联调测试跑（`crates/bnu-core/tests/gateway_live.rs`，客户端同一条代码路径）：

```bash
BNU_TEST_BASE_URL=https://chatapi.bnu.edu.cn/bnuapi BNU_TEST_API_KEY=sk-… \
  cargo test -p bnu-core --test gateway_live -- --ignored --nocapture --test-threads=1
```

| 调用 | 结果 |
|---|---|
| `GET /bnuapi/v1/models` | 200，16 个模型（含 `Qwen-Inno-35B-v1` / `qwen3-asr-1.7b` / `bge-m3` / `bge-reranker-v2-m3`） |
| chat（非流式，关思考） | 200，351 ms，content=`正常`，`reasoning_tokens=0` |
| chat（流式） | 200，496 ms（首字 290 ms），34 个增量 |
| chat（流式中断） | 200，`finish=cancelled`，已收到增量 3 个 |
| 设置页「测试连通性」四项 | chat 347 ms / embedding 127 ms（1024 维）/ rerank 114 ms（#0=0.9993，#1=0.0000）/ asr 118 ms |
| embedding `bge-m3` | 200，2 条 1024 维 |
| rerank `bge-reranker-v2-m3` | 200，相关文档 0.9913 / 无关 0.000016 |
| asr `qwen3-asr-1.7b`（0.5 s 静音 WAV） | 200，返回 `嗯。`（静音也能连通） |
| 旧前缀 `POST /v1/chat/completions` | 200（兼容保留） |

## 实测记录（2026-09-22，集群网关）

| 调用 | 结果 |
|---|---|
| `GET /v1/models` | 200，16 个模型 |
| chat `Qwen-Inno-35B-v1`（关思考，max_tokens=64） | 200，**377 ms**，content=`正常` |
| chat `Qwen-Inno-35B-v1`（默认思考，max_tokens=2048） | 200，2.3 s，653 tokens（含 1260 字思考） |
| embedding `bge-m3` | 200，132 ms，1024 维 |
| rerank `bge-reranker-v2-m3` | 200，169 ms，相关 0.9993 / 无关 0.0000 |
| ASR `qwen3-asr-1.7b`（13 s 中文语音 WAV） | 200，310 ms，转写与原文完全一致 |

对应自动化：`crates/bnu-core/tests/gateway_live.rs`（默认 `#[ignore]`）。

```bash
BNU_TEST_API_KEY=sk-xxx cargo test -p bnu-core --test gateway_live -- --ignored --nocapture
```

## 上游 ASR 限制（移植自既有会议项目实测）

- 只接受 **WAV**（mp3/m4a 会报 `Invalid or unsupported audio file`）；录音后统一转 WAV 再上传；
- 上游 vLLM：`max-model-len 32768`、音频多模态上限 16384、`max_new_tokens 4096`、`max-num-seqs 4`；
- 切段经验：单段 8 s~45 s（客户端默认 4 分钟整段，转写前按静音细切）、静音阈值 600 ms、RMS 自适应；
- 限流经验：约 9 请求/分钟，需要排队 + 退避重试。

## TLS 根证书（v0.3.3 修）

HTTP 客户端用 **系统证书库 + 内置 Mozilla 根证书**合并后的根证书库（`models::root_store()` / `models::tls_config()`）。

为什么要合并：`reqwest` 的 `rustls-tls-native-roots` 依赖 `rustls-native-certs` → `openssl-probe`，而
`openssl-probe` 在 `target_os = "android"` 分支里**只认 Termux 的路径**
（`/data/data/com.termux/files/usr/etc/tls/cert.pem`），APK 里既没有 Android 系统 CA 目录也没有 APEX 目录，
于是安卓上根证书数量是 **0**：所有 `https://` 请求都以
`invalid peer certificate: UnknownIssuer` 失败（Windows 读系统证书库，因此桌面端正常）。
现在再并一份内置根证书兜底，公网 CA（DigiCert / Let's Encrypt 等）都能验过。

- 自签 / 内网 CA 的 `https://`（如集群 `:9443` 的 vllm-tls）在两端都验不过——桌面端需要把 CA 装进系统证书库；
- 请求失败时的错误信息会带上 source 链（`请求失败: <url> → ... → invalid peer certificate: UnknownIssuer`），
  别再只写一句「请求失败」（`models::req_err`）；
- 自检：`cargo test -p bnu-core --lib -- --ignored --nocapture bundled_roots`
  （会真的打一次网关：只装内置根证书应通、空根证书库应报 `UnknownIssuer`）。

## 安全

- API Key 用本机密钥（`secret.key`，权限 0600，位于应用数据目录）AES-256-GCM 加密后存进 SQLite；
- 前端只能拿到 `hasKey: true/false`，不回显明文；保存时留空表示保持原 Key；
- 仓库内不出现真实 Key；开发用 Key 放 `.env.local`（被 `.gitignore` 的 `*.local` 覆盖）。

## 相关代码

- `crates/bnu-core/src/models.rs`：配置结构与 HTTP 客户端（chat / embed / rerank / transcribe / list / test）
- `crates/bnu-core/src/secret.rs`：本机密钥保险箱
- `src-tauri/src/lib.rs`：命令 `get_model_config` / `save_model_config` / `test_model` / `list_available_models`
- `src/views/SettingsView.vue`：四类端点设置页（预设、拉取模型列表、保存并测试）
