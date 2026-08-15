#!/usr/bin/env bash
# 服务端安装包：把 musl 静态二进制 + skills/ 目录打成 tar.gz，文件名含版本号与 commit。
# 前置：先运行 scripts/build-musl.sh 产出二进制。
# 产物：dist/xueban-server-<version>-<short_sha>.tar.gz（CI 上传为 artifact，可直接交付部署）。
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/x86_64-unknown-linux-musl/release/bootstrap
if [ ! -x "$BIN" ]; then
  echo "[package-server] 找不到 musl 二进制：$BIN（先运行 scripts/build-musl.sh）" >&2
  exit 1
fi

version=${VERSION:-$(awk -F'"' '/^version *=/{print $2; exit}' crates/bootstrap/Cargo.toml)}
short_sha=$(git rev-parse --short HEAD 2>/dev/null || echo local)

name="xueban-server-${version}-${short_sha}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/$name/skills"
cp "$BIN" "$tmp/$name/xueban"
cp -r skills/. "$tmp/$name/skills/"

mkdir -p dist
tar -C "$tmp" -czf "dist/$name.tar.gz" "$name"

echo "[package-server] 安装包：dist/$name.tar.gz"
