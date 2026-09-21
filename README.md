# BNU 会议纪要（bnu-meeting-notes）

面向高校会议场景的**本地化会议纪要产品**：手机/平板录音 → 分段实时转文字 → 一键生成结构化会议纪要 → 导出 Word。

- 模型能力全部来自集群（BNUAPI 网关 + Qwen3-ASR），**数据不出校园网**；
- 后端**无状态**：音频只在内存中流转，转成文字即丢弃，服务端不保存任何录音/文本；
- 录音与文本保存在手机本地，服务端不建数据库、不占存储。

## 仓库结构

```
bnu-meeting-notes/
├── backend/            # M1 已完成：无状态后端（FastAPI）
│   ├── app/
│   │   ├── api/        # 路由层（薄）：参数校验、鉴权、响应
│   │   ├── services/   # 业务层：转写、纪要、渲染、导出（纯逻辑，可单测）
│   │   ├── providers/  # 上游适配层：OpenAI 兼容实现 + 工厂（可替换）
│   │   ├── schemas/    # 请求/响应契约（pydantic）
│   │   ├── prompts/    # 提示词（外置，改文案不改代码）
│   │   └── core/       # 配置、错误、日志、提示词加载
│   ├── tests/          # 47 个单测（不依赖网络）
│   ├── scripts/        # dev.sh / smoke.sh / 样例转写文本
│   └── deploy/         # 单机部署（systemd + venv）与 Docker 备用方案
├── app/                # M3 已完成：Flutter App（Android）
│   ├── lib/            # core / data / domain / state / ui 五层（见 docs/APP.md）
│   └── test/           # WAV 工具、限流器、模型层单测 + 真实后端联调
└── docs/               # 架构、接口、部署、App、开发环境、真机回归清单（TESTING.md）
```

## 后端快速开始（本地）

```bash
cd backend
bash scripts/dev.sh                 # 建 venv + 装依赖 + 热重载启动
# 打开 http://127.0.0.1:8000/docs 查看接口
```

冒烟（需要一段 WAV 和 BNUAPI key）：

```bash
BASE_URL=http://127.0.0.1:8000 \
API_KEY=sk-xxxx \
SAMPLE_WAV=/path/to/segment.wav \
bash backend/scripts/smoke.sh
```

## 接口一览

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/health` | 健康检查 |
| GET | `/v1/defaults` | 默认 ASR/纪要模型 id |
| GET | `/v1/upstream/models` | 用当前 Key 拉上游模型列表（校验 Key，不耗额度） |
| POST | `/v1/segments/transcribe` | 上传一段 WAV → 返回该段文本 |
| POST | `/v1/minutes` | 全文 → 结构化纪要 + Markdown |
| POST | `/v1/minutes/docx` | 全文 → 直接返回 Word |
| POST | `/v1/export/docx` | 已编辑的纪要 JSON → Word |

统一鉴权：`Authorization: Bearer <用户自己的 BNUAPI key>`（服务端不保存）。
上游地址可用请求头 `X-Upstream-Base-URL` 覆盖（App 设置页的「BNUAPI 地址」）。

详见 [docs/API.md](docs/API.md)。

## 部署（单机 + systemd）

```bash
bash backend/deploy/deploy.sh       # rsync + 远端 venv/systemd 安装 + 健康检查
```

详见 [docs/DEPLOY.md](docs/DEPLOY.md)。

## 里程碑

- [x] **M1** 无状态后端：分段转写 + 纪要生成 + Word 导出（已在服务器部署）
- [x] **M2** Flutter App（Android）：录音分段、上传队列、实时文本、本地存档、Word 导出
- [x] **M3** 锁屏/后台录音（前台服务）、转写文本编辑、纪要抬头信息、分段试听与连播、上传限流（9 请求/分钟 + 429 退避）、正式签名 keystore
- [ ] **M4**（待定）音频拼接导出、会议列表增强、真流式；热词/PDF/低流量模式/说话人分离已明确不做

## App 快速开始

```bash
source ~/tools/env.sh     # JDK/Flutter/Android SDK（见 docs/DEV_ENV.md）
cd app
flutter build apk --release           # 产物 build/app/outputs/flutter-apk/app-release.apk
```

手机安装后：设置页填 **BNUAPI 地址 + Key** → 开始录音 → 结束 → 生成纪要 → 导出 Word。
详见 [docs/APP.md](docs/APP.md)。
