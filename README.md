# BNU Notes（本地优先的笔记 + 会议客户端）

Windows / Android 桌面客户端：**笔记与会议合并成一个「会议笔记」栏目**——vault 里每篇 Markdown
都能用文件夹树组织；开会时录音、用 `/v` 引用录音、一键转写 + 纪要。全部数据在本地，云端只当
"模型 API"用——在设置里填 baseURL + Key，客户端直连。

- 技术栈：**Tauri 2 + Vue 3 + TypeScript**，Rust 侧承载索引、加密与模型调用（`crates/bnu-core`）。
- 本地数据：vault（Markdown 文件）+ SQLite（FTS5 全文索引、图谱、设置）；API Key 用本机密钥 AES-256-GCM 加密存储。
- 优雅降级：未配置 embedding / rerank 时检索自动退化为全文搜索；未配置对话模型时只有问答与纪要不可用。
- 界面：TDesign 组件 + 语义 token，明暗两套主题（默认跟随系统，侧栏/设置页可切）；答案/笔记共用一套 Markdown 排版。
- 问答：真流式逐字输出（可随时停止）、回答里的 `[n]` 可点回原文定位、会话历史分组管理、按需生成追问建议。

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
crates/bnu-core/     核心库：db / graph / markdown / chunk / notes / models / secret
src-tauri/           Tauri 命令层（薄封装）
src/core/            前端类型与契约
src/platform/        适配器（Tauri 实现 / 浏览器 Mock）
src/stores/          Pinia
src/views/           会议笔记 / 问答 / 图谱 / 设置
docs/graph.md        知识图谱：抽取、存储、可视化与检索增强
docs/models.md       四类模型端点、实测记录与注意事项
```

## 文档

- [docs/graph.md](docs/graph.md)：知识图谱（实体/关系抽取、防幻觉约束、2D 力导 + 3D 星球视图、邻居扩展检索）。
- [docs/qa.md](docs/qa.md)：问答——流式生成与停止、行内引用定位、会话历史（`chat-history.json`）、追问建议与限流。
- [docs/models.md](docs/models.md)：模型端点配置、推荐模型、实测数据、上游限制。
- [docs/meetings.md](docs/meetings.md)：会议笔记——文件夹树、**会议 = 一篇 md**（`/v` 引用录音、一键处理转写 + 纪要）、录音归类、旧会议迁移与排障。
- [docs/windows.md](docs/windows.md)：Windows 安装包怎么出、安装、数据位置与验收清单。
- [docs/android.md](docs/android.md)：Android 工程、权限、release 签名与构建命令。
