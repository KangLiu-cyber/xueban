# 学伴 — 本地开发与门禁（docs/architecture.md §11）
# 用法：
#   make dev         本地一键启动后端（依赖本机 PostgreSQL）
#   make web         启动前端开发服务器（trunk，需 wasm32 target）
#   make desktop     启动桌面端（Tauri 壳，自动先起 trunk）
#   make gate        提交门禁：fmt + clippy + test + check 全绿
#   make build-musl  复现 CI：cross 交叉编译 musl 静态二进制
#   make package     复现 CI：打服务端安装包（tar.gz）
#   make start/stop  一键启动/停止部署（docker compose，见 scripts/start.sh）
#   make e2e         企业场景端到端冒烟（真实服务 + 真实 PG，见 scripts/e2e.sh）

SHELL := /bin/bash

DATABASE_URL ?= postgres://postgres@localhost/xueban?host=/tmp
BIND_ADDR ?= 127.0.0.1:8080

.PHONY: dev run web desktop gate check build-musl package start stop e2e

## 本地一键启动后端（连接串 / 监听地址可用环境变量覆盖）
dev run:
	DATABASE_URL="$(DATABASE_URL)" BIND_ADDR="$(BIND_ADDR)" cargo run -p bootstrap

## 启动前端开发服务器（trunk 编译 WASM + 热重载；缺 wasm target 时先跑
## `rustup target add wasm32-unknown-unknown`）。监听 127.0.0.1:8081，
## /api/v1 由 trunk 代理到后端 8080（见 clients/web/Trunk.toml），
## 可与 make dev 同时运行。
web:
	cd clients/web && trunk serve

## 启动桌面端（Tauri 壳；tauri.conf.json 已配 beforeDevCommand 自动先起 trunk）
desktop:
	cd clients/desktop && cargo tauri dev

## 提交门禁：与 CI gate job 一致（格式就地修复后跑其余三项）
gate:
	cargo fmt --all
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace
	cargo check --workspace

## 快速编译检查
check:
	cargo check --workspace

## 复现 CI：cross 交叉编译 x86_64-unknown-linux-musl 静态二进制
build-musl:
	scripts/build-musl.sh

## 复现 CI：打服务端安装包（tar.gz，含 skills/）
package:
	scripts/package-server.sh

## 一键启动/停止部署（docker compose；start 缺产物自动补齐）
start:
	scripts/start.sh

stop:
	scripts/stop.sh

## 企业场景端到端冒烟：真实服务进程 + 真实 PostgreSQL + 真实 HTTP，
## 覆盖体育学习闭环、鉴权、隔离、事件流与重启持久化（见 scripts/e2e.sh）
e2e:
	scripts/e2e.sh
