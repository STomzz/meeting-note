# Windows 桌面端（P6）

## 安装包怎么来的

本机（Linux/WSL）**不能**交叉打 Windows 包——`tauri build` 需要 MSVC 工具链 + NSIS/WiX；
所以走 GitHub Actions（`.github/workflows/ci.yml` 里的 `windows-build`）：

| 触发方式 | 结果 |
|---|---|
| 推送 `v*` 标签（推荐） | 自动构建，并把 `*-setup.exe` / `*.msi` 挂到该标签的 Release 上，直接下载 |
| Actions 页手动 `workflow_dispatch` | 只上传为 workflow artifact（需登录 GitHub 才能下） |

- runner：`windows-latest`（x64）；产物路径 `src-tauri/target/release/bundle/{nsis,msi}/`
- 版本号取 `src-tauri/tauri.conf.json`（当前 `0.1.1`），文件名形如 `bnu-notes_0.1.1_x64-setup.exe`
- 安装包内**不含任何密钥**：网关地址与 API Key 由用户在应用内「设置 → 模型能力」填写

## 安装

- **未做代码签名**：首次运行会触发 SmartScreen（「更多信息 → 仍要运行」）。内网自用可接受，
  要消除提示需买代码签名证书，并在 `tauri.conf.json` 配 `bundle.windows.certificateThumbprint`。
- NSIS 安装包默认**按当前用户安装**（`NSISInstallerMode::CurrentUser`，不需要管理员）。
- WebView2 运行时：Win11 自带；Win10 缺失时安装包会静默下载 bootstrapper（需要联网）。

## 数据放在哪

| 内容 | 路径 |
|---|---|
| 数据库 / 密钥 / 旧版会议录音 | `%APPDATA%\com.bnu.notes\`（`index.sqlite`、`secret.key`、`meetings\`） |
| 笔记库（vault） | 默认 `%USERPROFILE%\Documents\BNU-Notes`，设置页可改 |
| 会议录音（P7 起） | **vault 内** `<vault>\会议音频\<笔记名>\seg_0001.wav`，跟着笔记一起备份 |
| 会议转写缓存 | `index.sqlite` 表 `audio_transcripts`（按文件戳命中，重跑不重复烧 ASR） |

> 卸载**不会**删除上面两个目录；要清干净需手动删除。

## 麦克风（录音）

1. 应用侧：已注册 WebView2 `PermissionRequested`，放行本应用页面（`http(s)://tauri.localhost`、
   开发服务器的 localhost）的麦克风/摄像头，见 `src-tauri/src/webview_permissions.rs`。
2. 系统侧：**仍然需要**「设置 → 隐私和安全性 → 麦克风」里打开
   *让桌面应用访问你的麦克风*（以及应用列表里对应开关），否则 `getUserMedia` 直接 `NotAllowedError`。
3. 排障：会议页 → 选中一篇会议笔记 →「录音自检（排障用）」面板会显示每一步的错误名
   （`NotAllowedError` / `NotFoundError` / `NotReadableError`）。

## 验收清单（v1.3：会议 = 一篇 md）

1. 安装 → 启动 → 「设置 → 模型能力」：填网关地址 + Key，保存后点「测试连接」；
2. 笔记：新建 / 编辑 / 保存 → 列表出现 → 全文搜索命中；
3. 会议：**会议页 → 新建会议**（生成 `会议/<日期>-<标题>.md`）→ 右下角 🎙 面板「开始录音」→
   说 10 秒 → 「停止并插入引用」→ 笔记里出现 `/v 会议音频/…`，切「预览」能看到播放器并试听；
4. 处理：点「一键处理」→ 进度条走完 → 引用下方出现 `> 🎙 转写 …`，文件末尾出现
   「## 会议纪要（AI 整理）」正文；再点一次「一键处理」应显示「复用缓存」且转写块不重复堆叠；
5. 编辑器：在笔记里输入行首 `/v` 应弹出录音选择器（↑↓ + Enter 插入），工具栏有
   「一键处理（N 段录音）」；
6. 迁移：会议页详情「旧版会议」→「导出为笔记」→ `会议/旧会议/` 下出现旧会议笔记（含旧纪要）；
7. 互通：与 Android 之间用「导出 zip / 导入 zip」搬笔记（v1 手动，不用云）。

## 已知限制

- 只出 x64 安装包（`x86_64-pc-windows-msvc`）；未做 ARM64；
- 无自动更新（未接 `tauri-plugin-updater`）；
- 未签名 → SmartScreen 提示；
- Linux 桌面端未处理 WebKitGTK 媒体权限，录音只在 Windows / Android 验证过。
