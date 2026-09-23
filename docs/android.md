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

## 安装与验证清单

1. 把 APK 传到手机（`/mnt/c/Users/<你>/Desktop/` 里的副本直接拖进手机，或 `adb install -r bnu-notes.apk`）；
2. 允许「安装未知来源应用」；
3. 打开 App → 先在**设置**里填「API 地址 + Key」（与桌面端一致；手机需要能访问模型网关）；
4. **会议笔记** 页 → 新建 / 打开一篇笔记 → 右下角「录音」胶囊 → 展开面板选「记到」→「开始录音」→
   说话（胶囊上红点 + 计时 + 波形）→「停止录音」（不会自动插入引用）→ 点面板上的「插入引用」，
   或到笔记工具栏「N 段」/ 输入行首 `/v` 选择；切「预览」可试听（前台录音 + Wake Lock）；
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
- **https 网关（v0.3.2 及以前）**：安卓上根证书读不到（见 `docs/models.md` 的「TLS 根证书」），
  所有 https 请求都会失败（`invalid peer certificate: UnknownIssuer`）→ 用 v0.3.3 及以后的 APK；
  临时替代：把网关地址换成 `http://` 的内网地址（内网 IP 不受 TLS 影响）。
