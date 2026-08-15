#!/usr/bin/env bash
# 服务端部署包：musl 静态二进制 + skills/ + 前端 web 产物 + deploy/ 编排，
# 解压即部署源（docker compose -f deploy/docker-compose.yml up --build），
# 文件名含版本号与 commit。CI（.github/workflows）上传为 Release 资产。
# 前置：scripts/build-musl.sh（二进制）+ clients/web/dist（trunk build --release）。
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/x86_64-unknown-linux-musl/release/bootstrap
WEB=clients/web/dist
if [ ! -x "$BIN" ]; then
  echo "[package-server] 找不到 musl 二进制：$BIN（先运行 scripts/build-musl.sh）" >&2
  exit 1
fi
if [ ! -f "$WEB/index.html" ]; then
  echo "[package-server] 找不到前端产物：$WEB/index.html（先运行 cd clients/web && trunk build --release）" >&2
  exit 1
fi

version=${VERSION:-$(awk -F'"' '/^version *=/{print $2; exit}' crates/bootstrap/Cargo.toml)}
short_sha=$(git rev-parse --short HEAD 2>/dev/null || echo local)

name="xueban-server-${version}-${short_sha}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/$name"
cp -r deploy "$tmp/$name/deploy"
cp -r skills "$tmp/$name/skills"
mkdir -p "$tmp/$name/target/x86_64-unknown-linux-musl/release"
cp "$BIN" "$tmp/$name/target/x86_64-unknown-linux-musl/release/bootstrap"
mkdir -p "$tmp/$name/clients/web"
cp -r "$WEB" "$tmp/$name/clients/web/dist"

mkdir -p dist
tar -C "$tmp" -czf "dist/$name.tar.gz" "$name"

echo "[package-server] 部署包：dist/$name.tar.gz"
echo "[package-server] 部署：解压后 docker compose -f deploy/docker-compose.yml up -d --build"
