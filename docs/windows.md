# Windows 桌面端（P6）

## 安装包怎么来的

本机（Linux/WSL）**不能**交叉打 Windows 包——`tauri build` 需要 MSVC 工具链 + NSIS/WiX；
所以走 GitHub Actions（`.github/workflows/ci.yml` 里的 `windows-build`）：

| 触发方式 | 结果 |
|---|---|
| 推送 `v*` 标签（推荐） | 自动构建，并把 `*-setup.exe` / `*.msi` 挂到该标签的 Release 上，直接下载 |
| Actions 页手动 `workflow_dispatch` | 只上传为 workflow artifact（需登录 GitHub 才能下） |

- runner：`windows-latest`（x64）；产物路径 `src-tauri/target/release/bundle/{nsis,msi}/`
- 版本号取 `src-tauri/tauri.conf.json` 里的 `version`，文件名形如 `bnu-notes_<版本>_x64-setup.exe`
- 安装包内**不含任何密钥**：网关地址与 API Key 由用户在应用内「设置 → 模型能力」填写

## 安装

- **未做代码签名**：首次运行会触发 SmartScreen（「更多信息 → 仍要运行」）。内网自用可接受，
  要消除提示需买代码签名证书，并在 `tauri.conf.json` 配 `bundle.windows.certificateThumbprint`。
- **Smart App Control（智能应用控制）会直接拦未签名应用**：Win11 上如果「安全中心 → 应用和浏览器控制 →
  智能应用控制」处于开启状态，安装/运行可能被阻断且没有「仍要运行」。自用机器可在同一页面把它关掉
  （关闭后不可再打开，除非重装系统——自己权衡），或改用组策略/签名方案。
- NSIS 安装包默认**按当前用户安装**（`NSISInstallerMode::CurrentUser`，不需要管理员）。
- WebView2 运行时：Win11 自带；Win10 缺失时安装包会静默下载 bootstrapper（需要联网）。

## 数据放在哪

| 内容 | 路径 |
|---|---|
| 数据库 / 密钥 / 旧版会议录音 | `%APPDATA%\com.bnu.notes\`（`index.sqlite`、`secret.key`、`meetings\`） |
| 笔记库（vault） | 默认 `%USERPROFILE%\Documents\BNU-Notes`，设置页可改 |
| 会议录音（P8 起） | **vault 内** `<vault>\会议音频\<笔记路径>\seg_0001.wav`（如 `会议\周会.md` → `会议音频\会议\周会\`），跟着笔记一起备份；0.1.x 的旧目录仍会被列出 |
| 会议转写缓存 | `index.sqlite` 表 `audio_transcripts`（按文件戳命中，重跑不重复烧 ASR） |

> 卸载**不会**删除上面两个目录；要清干净需手动删除。

## 麦克风（录音）

1. 应用侧：已注册 WebView2 `PermissionRequested`，放行本应用页面（`http(s)://tauri.localhost`、
   开发服务器的 localhost）的麦克风/摄像头，见 `src-tauri/src/webview_permissions.rs`。
2. 系统侧：**仍然需要**「设置 → 隐私和安全性 → 麦克风」里打开
   *让桌面应用访问你的麦克风*（以及应用列表里对应开关），否则 `getUserMedia` 直接 `NotAllowedError`。
3. 排障：设置页 → 数据维护 →「录音自检」（检查环境 / 录 5 秒试听）会显示每一步的错误名
   （`NotAllowedError` / `NotFoundError` / `NotReadableError`）。

## 验收清单（v2.0：笔记与会议合并 + 文件夹）

1. 安装 → 启动 → 「设置 → 模型能力」：填网关地址 + Key，保存后点「测试连接」；
2. 目录树：左侧应显示文件夹（点开才看到笔记）；「新建文件夹」建 `项目/子目录` 这类多级目录；
   右键笔记「移动到…」后笔记换了文件夹，双击仍能打开；
3. 笔记：新建 / 编辑 / 保存（Ctrl+S）→ 树里出现 → 全文搜索命中；
4. 会议录音：打开一篇会议笔记（或新建）→ 右下角 🎙 面板选「记到」→ 开始录音 → 说 10 秒 →
   **停止**（不应自动插入引用）→ 工具栏出现「🎙 1 段 · 1 未引用」→ 输入行首 `/v` 从选择器插入
   引用（或点「插入全部未引用」）→ 预览里播放器可试听；
5. 处理：点「一键处理」→ 进度条走完 → 引用下方出现 `> 🎙 转写 …`，文件末尾出现
   「## 会议纪要（AI 整理）」正文；再点一次应显示「复用缓存」且转写块不重复堆叠；
6. 未引用提醒：再录一段不插入引用，直接点「一键处理」→ 弹窗提示未引用录音，
   试一次「插入并处理」、一次「忽略并继续」；
7. 音频归类：打开 vault 的 `会议音频\会议\<笔记名>\`，应能看到该笔记的 `seg_*.wav`；
8. 迁移：设置页 → 数据维护 →「把旧版会议导出为笔记」→ `会议/旧会议/` 下出现旧会议笔记（含旧纪要）；
9. 互通：与 Android 之间用「导出 zip / 导入 zip」搬笔记（v1 手动，不用云）——尚未实现（P8 余项）。

## 已知限制

- 只出 x64 安装包（`x86_64-pc-windows-msvc`）；未做 ARM64；
- 无自动更新（未接 `tauri-plugin-updater`）；
- 未签名 → SmartScreen 提示；
- Linux 桌面端未处理 WebKitGTK 媒体权限，录音只在 Windows / Android 验证过。
