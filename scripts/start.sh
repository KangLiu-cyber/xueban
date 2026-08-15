#!/usr/bin/env bash
# 一键启动部署（docker compose：postgres + server）。
# 前置产物缺失时自动补齐：musl 二进制（scripts/build-musl.sh）与前端 web 产物
# （cd clients/web && trunk build --release）；部署包解压后两者已在包内，直接启动。
# 生产注意：先设 POSTGRES_PASSWORD（或 deploy/.env），默认是 compose 里的开发口令。
# 用法：scripts/start.sh
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/x86_64-unknown-linux-musl/release/bootstrap
WEB=clients/web/dist
COMPOSE="docker compose -f deploy/docker-compose.yml"

if [ ! -x "$BIN" ]; then
  echo "[start] 缺少 musl 二进制：$BIN，先执行 scripts/build-musl.sh ..."
  scripts/build-musl.sh
fi
if [ ! -f "$WEB/index.html" ]; then
  echo "[start] 缺少前端产物：$WEB/index.html，先执行 trunk build --release ..."
  (cd clients/web && trunk build --release)
fi
# 附件挂载目录必须预先创建：容器内以非 root 用户（uid 10001）运行，
# Docker 自动创建的绑定挂载属 root，写入会失败（§8.1 附件端点）。
mkdir -p attachments

echo "[start] $COMPOSE up -d --build ..."
$COMPOSE up -d --build

cid=$($COMPOSE ps -q server 2>/dev/null || true)
if [ -n "$cid" ]; then
  echo -n "[start] 等待 server 健康检查"
  health=starting
  for i in $(seq 1 60); do
    health=$(docker inspect --format '{{.State.Health.Status}}' "$cid" 2>/dev/null || echo starting)
    [ "$health" = "healthy" ] && break
    echo -n "."
    sleep 1
  done
  echo
  if [ "$health" != "healthy" ]; then
    echo "[start] server 60s 内未就绪（当前 $health），日志：" >&2
    $COMPOSE logs --tail=50 server >&2 || true
    exit 1
  fi
fi

echo "[start] 已就绪：http://127.0.0.1:8080（MCP 端点见 MCP_ENDPOINT，默认 http://127.0.0.1:8080/mcp）"
