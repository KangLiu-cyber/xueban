# 学伴服务端镜像：Rust 多阶段构建 bootstrap 单二进制（docs/architecture.md §11）。
#
# 构建产物不含源码；migrations 经 sqlx::migrate! 编译期嵌入，运行时无需挂载。

# ---- builder ----
FROM rust:1.97-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release -p bootstrap

# ---- runtime ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/bootstrap /usr/local/bin/xueban
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["xueban"]
