#!/usr/bin/env bash
# 本地开发：创建 venv、安装依赖、热重载启动
set -euo pipefail

cd "$(dirname "$0")/.."

PYTHON_BIN="${PYTHON_BIN:-python3}"
VENV_DIR="${VENV_DIR:-.venv}"
PIP_INDEX="${PIP_INDEX:-https://pypi.tuna.tsinghua.edu.cn/simple}"

if [[ ! -d "$VENV_DIR" ]]; then
  echo "==> 创建虚拟环境 $VENV_DIR"
  "$PYTHON_BIN" -m venv "$VENV_DIR"
fi

echo "==> 安装依赖"
"$VENV_DIR/bin/pip" install -q -U pip -i "$PIP_INDEX"
"$VENV_DIR/bin/pip" install -q -e ".[dev]" -i "$PIP_INDEX"

echo "==> 启动开发服务（http://127.0.0.1:${MMA_PORT:-8000}/docs）"
exec "$VENV_DIR/bin/uvicorn" app.main:app --host "${MMA_HOST:-127.0.0.1}" --port "${MMA_PORT:-8000}" --reload
