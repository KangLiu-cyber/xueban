#!/usr/bin/env bash
# 学伴后端 systemd 部署安装脚本（docs/architecture.md §11「systemd 部署」）。
# 从仓库根或部署包解压目录运行，把 musl 二进制 + skills + 前端产物 + nginx 配置 +
# systemd 单元安装到宿主标准路径，并启动服务。与 docker compose 部署二选一。
#
# 前置产物（缺则自动补齐）：
#   - target/x86_64-unknown-linux-musl/release/bootstrap（scripts/build-musl.sh）
#   - clients/web/dist（cd clients/web && trunk build --release）
# 依赖：本机已装 PostgreSQL（DATABASE_URL 在安装后写入 /etc/xueban/xueban.env）；
#      可选已装 nginx（存在时安装 /etc/nginx/conf.d/xueban.conf）。
# 需 root 权限（创建系统用户、写 /etc、/usr/local/bin、/var）。
#
# 用法：sudo scripts/install-systemd.sh
set -euo pipefail
cd "$(dirname "$0")/.."

if [ "$(id -u)" -ne 0 ]; then
  echo "[install-systemd] 需要 root 权限：请用 sudo scripts/install-systemd.sh" >&2
  exit 1
fi

BIN=target/x86_64-unknown-linux-musl/release/bootstrap
WEB=clients/web/dist

if [ ! -x "$BIN" ]; then
  echo "[install-systemd] 缺少 musl 二进制：$BIN，先执行 scripts/build-musl.sh ..."
  scripts/build-musl.sh
fi
if [ ! -f "$WEB/index.html" ]; then
  echo "[install-systemd] 缺少前端产物：$WEB/index.html，先执行 trunk build --release ..."
  (cd clients/web && trunk build --release)
fi

# ---- 1. 系统用户与目录 ----
if ! id -u xueban >/dev/null 2>&1; then
  echo "[install-systemd] 创建系统用户 xueban ..."
  useradd --system --no-create-home --shell /usr/sbin/nologin xueban
fi

mkdir -p /usr/local/bin
install -m 0755 "$BIN" /usr/local/bin/xueban

mkdir -p /etc/xueban/skills
# skills/ 目录整体覆盖（含 references/），保留旧文件由宿主编排决定，这里清空重建保持一致
rm -rf /etc/xueban/skills/*
cp -r skills/. /etc/xueban/skills/
chown -R root:root /etc/xueban/skills

# 附件写目录（ReadWritePaths 放行目标）
mkdir -p /var/lib/xueban/attachments
chown -R xueban:xueban /var/lib/xueban/attachments
chmod 700 /var/lib/xueban/attachments

# ---- 2. 环境变量 ----
if [ ! -f /etc/xueban/xueban.env ]; then
  echo "[install-systemd] 生成 /etc/xueban/xueban.env（含占位符，务必编辑 DATABASE_URL 等）..."
  cp deploy/xueban.env.example /etc/xueban/xueban.env
fi
chmod 600 /etc/xueban/xueban.env

# ---- 3. systemd 单元 ----
install -m 0644 deploy/xueban.service /etc/systemd/system/xueban.service
systemctl daemon-reload

# ---- 4. 前端产物 + nginx（可选） ----
if command -v nginx >/dev/null 2>&1; then
  echo "[install-systemd] 检测到 nginx，安装前端产物与站点配置 ..."
  mkdir -p /var/www/xueban
  rm -rf /var/www/xueban/*
  cp -r "$WEB"/. /var/www/xueban/
  install -m 0644 deploy/nginx-systemd.conf /etc/nginx/conf.d/xueban.conf
  if nginx -t >/dev/null 2>&1; then
    systemctl reload nginx 2>/dev/null || nginx -s reload
  else
    echo "[install-systemd] 警告：nginx -t 未通过，站点配置已安装但未 reload，请检查后手动 reload" >&2
  fi
else
  echo "[install-systemd] 未检测到 nginx：跳过前端托管与反代配置（前端与 API 由外部网关另行处理）"
fi

# ---- 5. 启动服务 ----
systemctl enable xueban >/dev/null 2>&1 || true
systemctl restart xueban

echo "[install-systemd] 完成。"
echo "  - 服务：systemctl status xueban / journalctl -u xueban -f"
echo "  - 环境变量：编辑 /etc/xueban/xueban.env 后 systemctl restart xueban"
echo "  - 健康检查：curl http://127.0.0.1:8080/healthz"
if command -v nginx >/dev/null 2>&1; then
  echo "  - 对外访问：http://<host>（nginx 反代 /api/v1 与 /mcp）"
fi
