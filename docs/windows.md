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
| 数据库 / 密钥 / 会议音频 | `%APPDATA%\com.bnu.notes\`（`index.sqlite`、`secret.key`、`meetings\`） |
| 笔记库（vault） | 默认 `%USERPROFILE%\Documents\BNU-Notes`，设置页可改 |

> 卸载**不会**删除上面两个目录；要清干净需手动删除。

## 麦克风（录音）

1. 应用侧：已注册 WebView2 `PermissionRequested`，放行本应用页面（`http(s)://tauri.localhost`、
   开发服务器的 localhost）的麦克风/摄像头，见 `src-tauri/src/webview_permissions.rs`。
2. 系统侧：**仍然需要**「设置 → 隐私和安全性 → 麦克风」里打开
   *让桌面应用访问你的麦克风*（以及应用列表里对应开关），否则 `getUserMedia` 直接 `NotAllowedError`。
3. 排障：会议页「录音自检」面板会显示每一步的错误名（`NotAllowedError` / `NotFoundError` / `NotReadableError`）。

## 验收清单（v1）

1. 安装 → 启动 → 「设置 → 模型能力」：填网关地址 + Key，保存后点「测试连接」；
2. 笔记：新建 / 编辑 / 保存 → 列表出现 → 全文搜索命中；
3. 会议：新建会议 → 录音 10 秒 → 停止 → 分段可播放（走 `asset://` 协议）；
4. 转写：点「转写」，进度事件正常，结束后分段文本落库；
5. 纪要：生成后写入笔记库（`会议纪要/` 前缀的笔记）；
6. 互通：与 Android 之间用「导出 zip / 导入 zip」搬笔记（v1 手动，不用云）。

## 已知限制

- 只出 x64 安装包（`x86_64-pc-windows-msvc`）；未做 ARM64；
- 无自动更新（未接 `tauri-plugin-updater`）；
- 未签名 → SmartScreen 提示；
- Linux 桌面端未处理 WebKitGTK 媒体权限，录音只在 Windows / Android 验证过。
