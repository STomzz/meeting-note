#!/usr/bin/env bash
# 一键部署后端到服务器（rsync + 远端安装）
#
# 用法：bash backend/deploy/deploy.sh
set -euo pipefail

TARGET="${TARGET:-root@YOUR_SERVER_IP}"
REMOTE_DIR="${REMOTE_DIR:-/root/bnu-meeting-api}"

cd "$(dirname "$0")/.."
LOCAL_DIR="$(pwd)"

echo "==> 同步代码到 ${TARGET}:${REMOTE_DIR}/backend"
ssh "$TARGET" "mkdir -p '$REMOTE_DIR/backend'"
rsync -az --delete \
  --exclude '.venv' \
  --exclude '__pycache__' \
  --exclude '*.pyc' \
  --exclude '.pytest_cache' \
  --exclude '.ruff_cache' \
  --exclude 'smoke-out' \
  --exclude '.env' \
  --exclude '*.egg-info' \
  "$LOCAL_DIR/" "$TARGET:$REMOTE_DIR/backend/"

echo "==> 远端安装并重启服务"
ssh "$TARGET" "bash $REMOTE_DIR/backend/deploy/install.sh"

echo "==> 完成"
