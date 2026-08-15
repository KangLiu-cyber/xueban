# 学伴后端 — 本地开发与门禁（docs/architecture.md §11）
# 用法：
#   make dev         本地一键启动后端（依赖本机 PostgreSQL）
#   make gate        提交前门禁：fmt + clippy + test + check 全绿
#   make build-musl  复现 CI：cross 交叉编译 musl 静态二进制
#   make package     复现 CI：打服务端安装包（tar.gz）

SHELL := /bin/bash

DATABASE_URL ?= postgres://postgres@localhost/xueban?host=/tmp
BIND_ADDR ?= 127.0.0.1:8080

.PHONY: dev run gate check build-musl package

## 本地一键启动后端（连接串 / 监听地址可用环境变量覆盖）
dev run:
	DATABASE_URL="$(DATABASE_URL)" BIND_ADDR="$(BIND_ADDR)" cargo run -p bootstrap

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
