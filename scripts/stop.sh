#!/usr/bin/env bash
# 一键停止部署（docker compose down；pgdata 数据卷保留，重启数据仍在）。
# 彻底清库：docker compose -f deploy/docker-compose.yml down -v
set -euo pipefail
cd "$(dirname "$0")/.."

docker compose -f deploy/docker-compose.yml down
echo "[stop] 已停止（数据卷 pgdata 保留）"
