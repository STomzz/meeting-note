#!/usr/bin/env bash
# 出 Android release APK，并（可选）挂到 GitHub Release。
#
#   bash scripts/release-android.sh                 # 只构建：产物复制到 /tmp/opencode 与 Windows 桌面
#   bash scripts/release-android.sh v0.3.5          # 构建 + `gh release upload v0.3.5 <apk> --clobber`
#
# 说明：
# - 签名用 `src-tauri/gen/android/keystore.properties`（本机私密文件，勿入库）；
#   没有它 Gradle 会产出未签名包，安装时会被系统拒绝覆盖旧版本，务必确认文件存在。
# - 上传需要 gh CLI 已登录（`gh auth login`），或用 `GH_TOKEN=xxx` 环境变量。
set -euo pipefail

TAG="${1:-}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
APK_SRC="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk"
OUT_NAME="bnu-notes-${VERSION}.apk"

if [ ! -f src-tauri/gen/android/keystore.properties ]; then
  echo "⚠️  没找到 src-tauri/gen/android/keystore.properties：这次会产出未签名 APK" >&2
fi

# shellcheck disable=SC1091
. "$HOME/.cargo/env"
# shellcheck disable=SC1091
[ -f "$HOME/tools/env.sh" ] && . "$HOME/tools/env.sh"
export NDK_HOME="${ANDROID_HOME:-$HOME/tools/android-sdk}/ndk/28.2.13676358"

echo "==> 构建 Android APK（v${VERSION}）"
npm run tauri android build -- --apk --target aarch64

echo "==> 分发产物"
cp "$APK_SRC" "/tmp/opencode/${OUT_NAME}"
if [ -d /mnt/c/Users/91651/Desktop ]; then
  cp "$APK_SRC" "/mnt/c/Users/91651/Desktop/${OUT_NAME}"
fi
ls -l "$APK_SRC" "/tmp/opencode/${OUT_NAME}"
sha256sum "$APK_SRC"

if [ -n "$TAG" ]; then
  echo "==> 上传到 Release ${TAG}"
  gh release upload "$TAG" "$APK_SRC#${OUT_NAME}" --clobber
  gh release view "$TAG" --json assets -q '.assets[].name'
fi

echo "==> 完成：${OUT_NAME}"
