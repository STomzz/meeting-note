# BNU Notes（本地优先的笔记 + 会议客户端）

Windows / Android 桌面客户端：笔记（Markdown）、知识图谱、会议（录音 / 转写 / 纪要）全部在本地，
云端只当"模型 API"用——在设置里填 baseURL + Key，客户端直连。

- 技术栈：**Tauri 2 + Vue 3 + TypeScript**，Rust 侧承载索引、加密与模型调用（`crates/bnu-core`）。
- 本地数据：vault（Markdown 文件）+ SQLite（FTS5 全文索引、图谱、设置）；API Key 用本机密钥 AES-256-GCM 加密存储。
- 优雅降级：未配置 embedding / rerank 时检索自动退化为全文搜索；未配置对话模型时只有问答与纪要不可用。

## 开发

```bash
npm install
npm run dev            # 浏览器预览（Mock 适配器，不发真实请求）
npm run tauri dev      # 桌面客户端（真实命令层）
npm test               # 前端单测
npm run build          # 类型检查 + 前端构建

cargo test -p bnu-core                 # Rust 核心库单测
cargo test -p bnu-core -- --ignored    # 真实网关联调（需要 BNU_TEST_API_KEY，见 docs/models.md）
```

## 目录

```
crates/bnu-core/     核心库：db / markdown / chunk / notes / models / secret
src-tauri/           Tauri 命令层（薄封装）
src/core/            前端类型与契约
src/platform/        适配器（Tauri 实现 / 浏览器 Mock）
src/stores/          Pinia
src/views/           笔记 / 问答 / 图谱 / 会议 / 设置
docs/models.md       四类模型端点、实测记录与注意事项
```

## 文档

- [docs/models.md](docs/models.md)：模型端点配置、推荐模型、实测数据、上游限制。
- [docs/meetings.md](docs/meetings.md)：会议模块（录音 / 分段 / 转写 / 纪要）与平台麦克风权限。
- [docs/windows.md](docs/windows.md)：Windows 安装包怎么出、安装、数据位置与验收清单。
- [docs/android.md](docs/android.md)：Android 工程、权限、release 签名与构建命令。
