# 开发环境（用户态安装，无需 sudo）

开发机上没有系统级 JDK/Android SDK，全部装在 `~/tools/`：

| 组件 | 路径 / 版本 | 来源 |
|---|---|---|
| JDK 17 | `~/tools/jdk-17.0.20.1+1` | 清华 Adoptium 镜像 |
| Flutter SDK | `~/tools/flutter`（3.47.5 stable / Dart 3.13.4） | `storage.flutter-io.cn` |
| Android SDK | `~/tools/android-sdk`（platform-tools、android-36、build-tools 36.0.0） | `dl.google.com` |
| cmdline-tools | `~/tools/android-sdk/cmdline-tools/latest` | `dl.google.com` |

环境变量集中在 `~/tools/env.sh`，并已追加到 `~/.bashrc`：

```bash
source ~/tools/env.sh
```

内容：`JAVA_HOME` / `ANDROID_HOME` / `FLUTTER_ROOT` / `PATH`，
以及国内镜像 `FLUTTER_STORAGE_BASE_URL=https://storage.flutter-io.cn`、`PUB_HOSTED_URL=https://pub.flutter-io.cn`。

## 已处理的网络坑

| 问题 | 处理 |
|---|---|
| `maven.google.com` 不可达（curl 000） | `app/android/settings.gradle.kts` 与 `android/build.gradle.kts` 里把阿里云/腾讯 Maven 镜像放在 `google()` 之前 |
| HuggingFace / 官方 Flutter 源慢 | 用 `storage.flutter-io.cn` + `pub.flutter-io.cn`（env.sh 已设） |
| cmdline-tools 解压后无执行权限 | `chmod -R u+x android-sdk/cmdline-tools/latest/bin`（一次性） |
| Gradle 发行版 | `services.gradle.org` 可直连，无需镜像 |
| 插件 compileSdk 34 ↔ `flutter_plugin_android_lifecycle` 要求 36 | 把 `file_picker`(13)、`share_plus`(13)、`record`(7)、`wakelock_plus`(1.8) 升到当前大版本（老版本都还编译在 SDK 34 上，`checkReleaseAarMetadata` 会直接失败）；顺手移除了未使用的 `permission_handler`（`record` 自己申请麦克风权限） |
| file_picker 12+ API 变更 | `FilePicker.platform.pickFiles()` → `FilePicker.pickFile()` / `FilePicker.pickFiles()`（返回 `PlatformFile?` / `List<PlatformFile>`，不再有 `FilePickerResult`） |
| share_plus 11+ API 变更 | `Share.shareXFiles()` → `SharePlus.instance.share(ShareParams(files: [...]))` |
| 换插件版本后 Gradle 报 `Unresolved reference` / `cannot find symbol` | Kotlin 增量编译状态失效（插件的 `build/<plugin>` 目录里留有旧版本产物）。处理：`cd app && flutter clean`，或对报错模块单独重跑：`cd android && ./gradlew :share_plus:compileReleaseKotlin --rerun-tasks`（本次即用后者修好） |

## 正式签名（release keystore）

APK 默认回退到 Android debug key 签名（内测够用）；正式分发/上架用 release keystore：

| 项 | 位置 |
|---|---|
| keystore | `~/keys/bnu-meeting-release.jks`（PKCS12 / RSA 2048 / 有效期 10000 天 / alias `bnu-meeting`） |
| 口令与信息 | `~/keys/bnu-meeting-release.properties`（chmod 600） |
| 构建配置 | `app/android/key.properties`（已被 `android/.gitignore` 忽略，**不入库**） |

`android/app/build.gradle.kts` 检测到 `key.properties` 就用正式签名，否则自动回退 debug 签名
（新克隆/新机器不配也能构建，只是签的是 debug key）。构建脚本会打印实际签名证书：

```bash
cd app && bash scripts/build_apk.sh     # analyze + test + build + 复制到 dist/ + 打印证书
```

> **备份**：`.jks` 与口令文件必须离线备份——丢了就无法给已安装用户升级（只能卸载重装，本地数据清空）。
> **换签名**：签名不同的 APK 不能覆盖安装，必须先卸载旧版；因此第一次从 debug 版切到正式版时，
> 请先在旧版里把需要的会议导出 Word。

## 本地私有配置（后端地址）

App 里内置的后端默认地址通过**编译期注入**，真实地址不入库：

```bash
cd app
cp scripts/local.env.example scripts/local.env   # local.env 已被 .gitignore 忽略
# 编辑 local.env：MMA_BACKEND_BASE_URL=http://<你的后端>:8000
bash scripts/build_apk.sh                        # 脚本自动读 local.env 并注入
```

不配 `local.env` 时默认 `http://127.0.0.1:8000`（本机开发）；也可以在 App 的「高级设置」里随时手改。

## 常用命令

```bash
source ~/tools/env.sh
flutter doctor                 # 体检（Android toolchain 应打勾）
sdkmanager --list_installed    # 已装 SDK 组件
cd ~/meeting-note/app && flutter build apk --release
```

> 新建 Flutter 工程时同样要加 Maven 镜像，否则依赖会卡在 `maven.google.com`。
