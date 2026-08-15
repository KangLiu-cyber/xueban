# 运行镜像：只 COPY 预编译的 musl 静态二进制与 skills/ 目录，镜像内不编译。
#
# 二进制来源：CI（.github/workflows）用 cross 编译 `x86_64-unknown-linux-musl` 的
# bootstrap crate，产物按本文件假设的路径 `target/x86_64-unknown-linux-musl/release/bootstrap`
# 提供（本地 build 镜像前先跑 scripts/build-musl.sh）。SQL 迁移在编译期由
# sqlx::migrate! 嵌入二进制，运行时无需挂载 migrations/。

FROM alpine:3.20

RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -u 10001 xueban

WORKDIR /app

COPY target/x86_64-unknown-linux-musl/release/bootstrap /usr/local/bin/xueban
COPY skills /app/skills

ENV SKILLS_DIR=/app/skills \
    BIND_ADDR=0.0.0.0:8080

USER xueban
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/xueban"]
