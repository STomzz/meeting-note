# Android（Tauri 移动端）

目标：先用一个可安装的 APK 验证**前台录音链路**（权限 → WebView 采集 PCM → 落盘 → 回放 → 转写），
风险早暴露；不追求 Play 上架、不做后台录音。

## 环境（本机已具备）

| 组件 | 位置 |
| --- | --- |
| JDK 17 | `~/tools/jdk-17.0.20.1+1`（`~/tools/env.sh` 里设置 `JAVA_HOME`） |
| Android SDK | `~/tools/android-sdk`（`ANDROID_HOME`） |
| NDK | `~/tools/android-sdk/ndk/28.2.13676358`（`NDK_HOME`） |
| Rust 目标 | `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`（国内可用 `RUSTUP_DIST_SERVER=https://rsproxy.cn`） |

一次性初始化（已执行，工程在 `src-tauri/gen/android/`，已入库）：

```bash
source ~/.cargo/env && source ~/tools/env.sh
export NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
npm run tauri android init          # 生成 gen/android 工程
```

## 权限

`src-tauri/gen/android/app/src/main/AndroidManifest.xml`：

```xml
<uses-permission android:name="android.permission.RECORD_AUDIO" />
<uses-permission android:name="android.permission.MODIFY_AUDIO_SETTINGS" />
<uses-feature android:name="android.hardware.microphone" android:required="false" />
```

运行时授权由 wry 的 `RustWebChromeClient.onPermissionRequest` 处理：WebView 里第一次调用
`getUserMedia({audio:true})` 时会弹出系统授权框（`AUDIO_CAPTURE` → `RECORD_AUDIO`）。
拒绝后需要在系统设置里手动开启，App 内会显示「没有麦克风权限」。

## 构建 APK

debug 包（190 MB 左右，含调试符号，只用于快速验证）：

```bash
npm run tauri android build -- --debug --apk --target aarch64
# 产物：src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

release 包（~11 MB，已开启混淆；**推荐装这个**）：

```bash
npm run tauri android build -- --apk --target aarch64
# 产物：src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```

签名（release）：

- 密钥库在 `~/.android/bnu-notes.keystore`（密码存 `~/.android/bnu-notes-keystore.txt`，权限 600，
  **不要入库**；正式发布请换成你自己的密钥库并妥善备份——丢了就再也无法覆盖安装升级）；
- `src-tauri/gen/android/keystore.properties`（已 gitignore）指向密钥库，Gradle 会自动签名；
- 文件不存在时 release 构建产出未签名 APK，可用 `apksigner` 手动签：

```bash
$ANDROID_HOME/build-tools/35.0.0/apksigner sign --ks ~/.android/bnu-notes.keystore \
  --out bnu-notes.apk app-universal-release.apk
```

产物已确认：ABI 仅 `arm64-v8a`，签名证书 `CN=BNU Notes`，`libbnu_notes_lib.so` 为**生产模式**
（内嵌前端资源，不依赖 `localhost:1420` 开发服务器）。

## 图标

v0.3.5 起换成新图标（z-image-turbo 生成 → 处理成三张源图 → `tauri icon` 一把出全平台）：

- 源图与 manifest 放在仓库外（临时目录即可），结构是：
  `{ "default": "app-icon.png", "android_bg": "app-icon-bg.png", "android_fg": "app-icon-fg.png", "android_fg_scale": 100 }`；
- 重新生成：

```bash
npx tauri icon /path/to/manifest.json
# → src-tauri/icons/*（Windows .ico/.png、macOS .icns）
# → src-tauri/gen/android/app/src/main/res/mipmap-*/（ic_launcher、_round、_foreground、_background）
# → mipmap-anydpi-v26/ic_launcher.xml（自适应图标：背景层 + 前景层）
```

- 安卓自适应图标三件套说明：**背景层**（渐变满铺）+**前景层**（白页 + 声波，内容约占 62%，落在 66% 安全区内）
  + `mipmap-anydpi-v26/ic_launcher.xml` 引用两者；旧版 `ic_launcher.png`（API < 26）用同一张满铺方图。

## Release 里带 APK

两条路，任选：

**① 本地出包 + 上传（立刻可用）**

```bash
gh auth login                      # 一次性：需要 repo 权限
bash scripts/release-android.sh v0.3.5
# 构建 → 复制到 /tmp/opencode 与桌面 → `gh release upload v0.3.5 bnu-notes-0.3.5.apk --clobber`
```

**② CI 自动出包（加一次 secrets，之后每个 tag 自动挂）**

在仓库 Settings → Secrets and variables → Actions 加 4 个 secrets：

| secret | 取值 |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | `base64 -w0 ~/.android/bnu-notes.keystore` 的输出 |
| `ANDROID_KEYSTORE_PASSWORD` | `keystore.properties` 里的 `storePassword` |
| `ANDROID_KEY_ALIAS` | `keystore.properties` 里的 `keyAlias` |
| `ANDROID_KEY_PASSWORD` | `keystore.properties` 里的 `keyPassword` |

配好后推 `v*` tag：`windows-build` 出安装包、`android-build` 出 APK，两个 job 把资产挂到同一个 Release
（APK 资产名 `bnu-notes-<版本>.apk`，比如 `bnu-notes-0.3.6.apk`）。

> 现状（2026-09-23）：4 个 secrets 已配置；`android-build` 已用**手动触发**跑通过一次，产出的 APK
> 用 `apksigner verify --print-certs` 核对过证书 = `CN=BNU Notes`、SHA-256 指纹与本地出包一致
> （覆盖安装不会因签名不同失败）。手动再验一次：

```bash
gh workflow run ci.yml --ref client      # 手动触发（只出 artifact，不建 Release）
gh run watch                             # 或到 Actions 页面看
```

CI 里 Android 环节的小坑：不要用 `android-actions/setup-android@v3`（实测在 runner 上必失败），
现在是显式用 runner 自带的 `$ANDROID_SDK_ROOT`，缺了才下载 cmdline-tools；构建日志会 tee 到文件，
失败时自动把尾部 300 行推到 `ci-logs` 分支方便排查。
**缺 secret 时 android-build 自动跳过**（不会让 Release 失败）；密钥库仍然只存在你本机 + GitHub secrets，不入库。

## 安装与验证清单

1. 把 APK 传到手机（`/mnt/c/Users/<你>/Desktop/` 里的副本直接拖进手机，或 `adb install -r bnu-notes.apk`）；
2. 允许「安装未知来源应用」；
3. 打开 App → 先在**设置**里填「API 地址 + Key」（与桌面端一致；手机需要能访问模型网关）；
4. **会议笔记** 页 → 新建 / 打开一篇笔记 → 右下角「录音」胶囊 → 展开小条选目标笔记 →「开始」→
   说话（红点 + 计时 + 电平；收起后胶囊上也有计时 + 波形）→「停止」（不会自动插入引用）→ 点小条上的「插入引用」，
   或到笔记工具栏「N 段」/ 输入行首 `/v` 选择；引用行本身就是播放器，点一下就能试听（前台录音 + Wake Lock）；
   编辑后约 0.8 秒自动保存（标题栏出现「已保存」角标）；
4.1 触屏说明：树干上的「＋ / …」操作按钮在手机上常驻显示（没有 hover）；**长按**笔记可弹右键菜单
   （重命名 / 移动到…）；文件树拖拽只在桌面可用，手机请用「移动到…」；
5. 排障：设置页 → 数据维护 →「录音自检」：
   - 「检查环境」：应显示支持 `getUserMedia`、支持 Wake Lock；
   - 「录 5 秒试听」：第一次会弹麦克风授权 → 录完显示采样率/峰值，能播放说明采集链路可用；
6. 有网络时点「一键处理」→ 转写块 + 会议纪要写入同一篇 md（没有网络时录音与播放不受影响）；
6.1 问答：回答下面点「打开定位」会跳到笔记并高亮对应行；引用行在手机上可横向阅读；
6.2 主题：设置好后跟随系统深浅色；设置页「外观」或侧栏左下角按钮可手动三态切换；
6.3 问答：回答逐字出现，生成中可「停止生成」（保留已生成部分）；标题栏左侧「历史」按钮打开会话抽屉
    （窄屏默认收起），可改名/置顶/删除；点 `[n]` 角标或来源行跳回笔记并高亮；「猜你想问」生成追问建议；
7. 音频归类：录音存在 vault 的 `会议音频/<笔记路径>/` 下，可到文件管理器里核对。

## 已知限制与注意

- **只做前台录音**：息屏、切后台、被电话/其它 App 抢占麦克风都可能中断录音（v1 不做音频焦点与前台服务）；
- 录音期间会申请 `screen` Wake Lock 保持屏幕常亮（失败不阻断录音）；
- Android 上不能用 `localhost` 访问模型网关，必须填手机网络能到达的地址；
- `tauri android dev`（开发服务器 + 热重载）在 Tauri 里走 `tauri://localhost` 代理，
  需要 `adb reverse`；本仓库的验证流程用打包 APK，不走这条路径；
- 手机与电脑不在同一网络时，转写/纪要会失败（录音、播放、保存都是本地的，不受影响）；
- **系统栏遮挡（v0.3.3 及以前）**：`MainActivity` 开了 `enableEdgeToEdge()`（Android 15+ 对 targetSdk 35+ 本来就是强制的），
  而网页侧拿不到这些 Insets，设置页顶部的「推荐配置」等会被状态栏挡住一点、底部压手势条 →
  v0.3.4 起在 `MainActivity.onCreate` 把系统栏 + 挖孔尺寸补成内容视图的内边距（整页让开系统栏），
  并把 `android:windowBackground` 对齐应用背景色（`values/themes.xml` 浅色 `#F4F5F7`、`values-night/themes.xml` 深色 `#16181C`），
  免得状态栏背后露出突兀的色条；
- **https 网关（v0.3.2 及以前）**：安卓上根证书读不到（见 `docs/models.md` 的「TLS 根证书」），
  所有 https 请求都会失败（`invalid peer certificate: UnknownIssuer`）→ 用 v0.3.3 及以后的 APK；
  临时替代：把网关地址换成 `http://` 的内网地址（内网 IP 不受 TLS 影响）。
