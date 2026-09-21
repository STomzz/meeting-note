#!/usr/bin/env bash
# 在服务器上安装/升级后端（由 deploy.sh 调用，也可手动执行）
set -euo pipefail

APP_DIR="${APP_DIR:-/root/bnu-meeting-api/backend}"
PIP_INDEX="${PIP_INDEX:-https://pypi.tuna.tsinghua.edu.cn/simple}"

cd "$APP_DIR"

if [[ ! -d .venv ]]; then
  echo "==> 创建虚拟环境"
  python3 -m venv .venv
fi

echo "==> 安装依赖"
.venv/bin/pip install -q -U pip -i "$PIP_INDEX"
.venv/bin/pip install -q -e . -i "$PIP_INDEX"

echo "==> 配置文件"
mkdir -p /etc/bnu-meeting-api
if [[ ! -f /etc/bnu-meeting-api/env ]]; then
  install -m 600 deploy/env.example /etc/bnu-meeting-api/env
  echo "已生成 /etc/bnu-meeting-api/env（请按需修改后重启服务）"
fi

echo "==> systemd 服务"
install -m 644 deploy/bnu-meeting-api.service /etc/systemd/system/bnu-meeting-api.service
systemctl daemon-reload
systemctl enable --now bnu-meeting-api
systemctl restart bnu-meeting-api
sleep 2

systemctl --no-pager --lines=5 status bnu-meeting-api || true
echo "==> 健康检查"
curl -sf http://127.0.0.1:8000/health && echo
