# 部署（单机 + systemd）

后端与 BNUAPI 同机，通过 `http://127.0.0.1:3000/v1` 访问模型；服务本身**无状态、无数据库、不落盘**。

## 方式一：venv + systemd（当前正式方案）

本地执行一键部署：

```bash
bash backend/deploy/deploy.sh
```

脚本做了三件事：

1. `rsync` 代码到 `root@YOUR_SERVER_IP:/root/bnu-meeting-api/backend`
2. 远端创建 venv、安装依赖（走清华 pip 镜像）、安装 systemd 服务
3. 重启服务并做 `/health` 健康检查

首次部署后会生成配置文件 `/etc/bnu-meeting-api/env`（权限 600）：

```ini
MMA_UPSTREAM_BASE_URL=http://127.0.0.1:3000/v1
MMA_UPSTREAM_API_KEY=          # 兜底 key，建议留空由 App 传
MMA_ALLOW_SERVER_KEY=true
MMA_DEFAULT_ASR_MODEL=qwen3-asr-1.7b
MMA_DEFAULT_MINUTES_MODEL=Qwen-Inno-35B-v1
```

修改配置后 `systemctl restart bnu-meeting-api`。

常用命令：

```bash
systemctl status bnu-meeting-api
journalctl -u bnu-meeting-api -f
curl -s http://127.0.0.1:8000/health
```

## 方式二：Docker（备用）

服务器上 Docker Hub 不可达（`registry-1.docker.io` 返回 000），需要先通过镜像站/其他节点导入 `python:3.12-slim`，然后：

```bash
cd /root/bnu-meeting-api/backend
docker compose up -d --build
```

## 网络与端口

| 项 | 值 |
|---|---|
| 监听 | `0.0.0.0:8000`（服务器上 8000 未被占用） |
| 上游 | `127.0.0.1:3000`（new-api） |
| App 访问 | 内网 `http://YOUR_SERVER_IP:8000`；若需 HTTPS，可在前面加一层 nginx 反向代理 |

## 冒烟验证

在本地或服务器上：

```bash
BASE_URL=http://YOUR_SERVER_IP:8000 \
API_KEY=sk-xxxx \
SAMPLE_WAV=/path/to/segment.wav \
bash backend/scripts/smoke.sh
```

产物：`smoke-out/segment.json`、`minutes.json`、`minutes.docx`。
