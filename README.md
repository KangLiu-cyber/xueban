# 学伴 · 超级学习助手

备考学习助手：**学习空间**（AI 生成的目录与笔记树）、**刷题与错题本**、**组卷模考**、**复盘 Agent**。Agent 侧（TRAE 等）经 MCP 接入生成内容，服务端零 LLM 调用。

## 功能

| 模块 | 说明 |
|:---|:---|
| 学习空间 | AI 生成目录与笔记树，支持图片附件（Agent 经 MCP 上传） |
| 刷题与错题本 | 练习、错题记录、掌握度标记 |
| 组卷模考 | 组卷与模拟考试 |
| 复盘 Agent | 经 MCP over HTTPS 接入，用户 token 鉴权 |

## 技术栈

- **服务端**：Rust（Axum + SQLx）+ PostgreSQL 16，六边形架构 + DDD（`domain` 零框架依赖）
- **网页 / 桌面端**：Rust（Leptos 编译 WASM）一套 UI 双形态——浏览器 + Tauri 壳
- **安卓端**：Kotlin + Jetpack Compose 独立实现，与 Rust 前端仅共用后端 API
- **Agent 接入**：MCP over HTTPS，Bearer token 鉴权

## 工程结构

```
├── Cargo.toml            # 后端 workspace（members = crates/*，exclude = clients）
├── crates/               # 后端六边形架构
│   ├── domain/           # 领域层：零框架依赖
│   ├── application/      # 应用层：用例编排
│   ├── adapter-http/     # Axum REST API（/api/v1）
│   ├── adapter-mcp/      # MCP 网关 + 能力下发
│   ├── adapter-postgres/ # SQLx 仓储与事件存储
│   └── bootstrap/        # 组装：依赖注入、配置、迁移、main
├── clients/              # 客户端：独立 workspace（被后端 exclude）
│   ├── web/              # Leptos 应用（lib crate，编译 WASM）
│   ├── desktop/          # Tauri 壳（bin crate，依赖 web）
│   └── android/          # Kotlin + Compose（Android Studio 项目）
├── migrations/           # SQL 迁移脚本
├── deploy/               # Docker Compose 部署编排
├── scripts/              # 构建 / 部署脚本
└── docs/                 # 架构 / 需求文档与 UI 原型
```

## 本地开发

依赖本机 PostgreSQL 16：

```sh
make dev                             # 一键启动后端（REST + MCP，127.0.0.1:8080）
make web                             # 启动前端开发服务器（trunk，需 wasm32 target）
make desktop                         # 启动桌面端（Tauri 壳，自动先起 trunk）
make gate                            # 提交门禁：fmt + clippy + test + check 全绿
```

安卓端用 Android Studio 打开 `clients/android`（gradle wrapper 已就位，需 JDK）。

## 构建与发布

- 服务端部署包：`make build-musl && make package`（cross 编译 musl 静态二进制 + 打包，与 CI 一致）
- **CI 只在打固定版本 tag 时触发**：推 `v0.1.0` 这类 `v*` tag 即跑整条流水线（gate → 三端构建 → GitHub Release），发布**三个独立交付物**——服务端部署包（tar.gz，docker 部署源）、安卓 APK、桌面端安装包（Linux .deb / .AppImage）。日常推分支不触发 CI。
- 客户端生产端点域名由 CI 注入（`-PapiBaseUrl` / `XUEBAN_API_BASE`），本地开发默认走本地回环。

## 文档

| 文件 | 说明 |
|:---|:---|
| `docs/requirements.md` | 需求文档（API 清单、限界上下文） |
| `docs/architecture.md` | 架构文档（§5.4 工程结构为权威目录定义） |
| `docs/ui-mockup/desktop-prototype-v3-soft.html` | 桌面端 v3 柔和版原型（已定稿） |
| `docs/ui-mockup/android-prototype-v1.html` | 安卓端原型 v1（已定稿） |
