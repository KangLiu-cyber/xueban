# 前端 web 镜像：只 COPY trunk build 产物（clients/web/dist），不编译。
# nginx 静态服务 + 同域反代 /api 与 /mcp 到 server 容器（deploy/nginx.conf），
# 前端 API 走同源兜底，无需注入 XUEBAN_API_BASE。
#
# 产物来源：CI（.github/workflows）或本地 `cd clients/web && trunk build --release`，
# 按本文件假设的路径 clients/web/dist 提供。

FROM nginx:1.27-alpine

COPY clients/web/dist /usr/share/nginx/html
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80
