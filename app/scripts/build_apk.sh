#!/usr/bin/env bash
# 一键构建 Android APK（用户态工具链，见 docs/DEV_ENV.md）
#
# 签名：app/android/key.properties 存在则用正式 release keystore，否则回退 debug 签名。
# 后端地址：app/scripts/local.env 里的 MMA_BACKEND_BASE_URL 会注入 APK 默认值（见 local.env.example）。
set -euo pipefail

# 本机工具链环境（JDK/Flutter/Android SDK）；没有这个文件时按 PATH 里的 flutter 走
if [ -f "$HOME/tools/env.sh" ]; then
  # shellcheck disable=SC1091
  source "$HOME/tools/env.sh"
fi

cd "$(dirname "$0")/.."

# 本地私有配置（不入库）
if [ -f scripts/local.env ]; then
  # shellcheck disable=SC1091
  source scripts/local.env
fi

flutter pub get
flutter analyze
flutter test

if [ -n "${MMA_BACKEND_BASE_URL:-}" ]; then
  echo "注入默认后端地址：$MMA_BACKEND_BASE_URL"
  flutter build apk --release --dart-define=MMA_BACKEND_BASE_URL="$MMA_BACKEND_BASE_URL"
else
  echo "未设置 MMA_BACKEND_BASE_URL，APK 默认后端地址为 http://127.0.0.1:8000"
  flutter build apk --release
fi

APK=build/app/outputs/flutter-apk/app-release.apk
BT=$(ls -d "$ANDROID_SDK_ROOT"/build-tools/* | tail -1)

echo
echo "APK 产物："
ls -lh "$APK"

echo
echo "签名证书："
"$BT/apksigner" verify --print-certs "$APK" \
  | grep -E "Signer #1 certificate (DN|SHA-256)" || true

VERSION=$(grep -m1 '^version:' pubspec.yaml | awk '{print $2}' | cut -d+ -f1)
mkdir -p ../dist
cp "$APK" "../dist/bnu-meeting-$VERSION.apk"
echo
echo "已复制到 dist/bnu-meeting-$VERSION.apk"
