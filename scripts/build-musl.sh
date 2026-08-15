#!/usr/bin/env bash
# cross 交叉编译 x86_64-unknown-linux-musl 静态二进制（后端 bootstrap）。
# CI（.github/workflows/ci.yml）与本地 `make build-musl` 共用。
# 产物：target/x86_64-unknown-linux-musl/release/bootstrap
#   —— deploy/Dockerfile 与 scripts/package-server.sh 都依赖该路径。
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cross >/dev/null 2>&1; then
  echo "[build-musl] 未安装 cross，执行 cargo install cross --locked ..."
  cargo install cross --locked
fi

cross build --release --target x86_64-unknown-linux-musl -p bootstrap

echo "[build-musl] 产物：target/x86_64-unknown-linux-musl/release/bootstrap"
