# 备用容器化方案（服务器上 Docker Hub 不可达，当前正式部署走 venv + systemd，见 deploy/install.sh）
FROM python:3.12-slim

WORKDIR /app

COPY pyproject.toml ./
COPY app ./app

RUN pip install --no-cache-dir -i https://pypi.tuna.tsinghua.edu.cn/simple .

EXPOSE 8000

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
