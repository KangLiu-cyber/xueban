# 学伴（超级学习助手）— 代理指南

面向 AI 代理的项目协作说明。核心信息以 `docs/` 下两份文档为准，本文件只提炼代理工作需要的约定。

## 项目概述

备考学习助手：学习空间（AI 生成的目录与笔记树）、刷题与错题本、组卷模考、复盘 Agent。Agent 侧（TRAE 等）经 MCP 接入生成内容；服务端零 LLM 调用。

- 服务端：Rust（Axum + SQLx）+ PostgreSQL 16，六边形架构 + DDD
- 客户端：Rust（Leptos 编译 WASM）一套 UI 双形态——网页端 + 桌面 Tauri 壳；安卓端 Kotlin + Jetpack Compose 独立实现
- Agent 接入：MCP over HTTPS，用户 token 鉴权

## 文档

| 文件 | 说明 |
|:---|:---|
| `docs/requirements.md` | 需求文档 v7.2（最终版），API 清单、限界上下文 |
| `docs/architecture.md` | 架构文档 v1.1，§5.4 工程结构为权威目录定义 |
| `docs/ui-mockup/desktop-prototype-v3-soft.html` | 桌面端 v3 柔和版原型（已定稿） |
| `docs/ui-mockup/android-prototype-v1.html` | 安卓端原型 v1（已定稿） |

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
└── docs/                 # 架构 / 需求文档与 UI 原型
```

## 构建与验证

```sh
cargo check --workspace            # 后端（仓库根目录）
cd clients && cargo check --workspace   # 前端 Rust（web + desktop）
```

- 安卓端不参与 cargo workspace，用 Android Studio 打开 `clients/android`（gradle wrapper 已就位，需 JDK）
- `clients/` 独立 workspace：wasm target 与 Tauri 原生依赖不拖累后端编译循环

## 架构约定

- 依赖方向：`bootstrap → adapter-* → application → domain`；`domain` 不依赖 workspace 内任何其他 crate，CI 用 cargo deny 守护纯净边界
- 客户端 API、MCP 协议、存储实现可独立替换（依赖倒置）
- UI 代码只有一份（`web`），编译 WASM 供浏览器与 Tauri 共用；`desktop` 仅是薄壳
- 安卓端与 Rust 前端独立，仅共用后端 API；按 `android-prototype-v1.html` 实现，WindowSizeClass ≥600dp 双栏
- 数据按用户严格隔离：user_id 来自 token 解析的上下文，不接受客户端声明
- API 兼容：变更走新增字段不删字段（两端共用 /api/v1）

## 开发约定

- **文档先行**：实现必须与 `docs/architecture.md`、`docs/requirements.md` 一致；需求或结构变更先更新文档，再改代码，文档与代码不得背离
- **UI 严格按原型**：前端页面开发必须与 `docs/ui-mockup/` 原型一致，布局、配色、字号、间距、文案、交互以原型为准，做到一模一样；原型是页面验收的唯一基准——web/desktop 按 `desktop-prototype-v3-soft.html`（v3 柔和版），android 按 `android-prototype-v1.html` 实现
- 工程结构变更先更新 `docs/architecture.md` §5.4，保持文档与磁盘一致
- §5.4 树中列出的模块文件（如 `domain/src/ports.rs`、`application/src/auth.rs`）在实现各层时创建，不建空壳占位
- **提交门禁**：只有 `cargo check`、`cargo clippy`（-D warnings）、`cargo test`、`cargo fmt` 全部通过，才允许提交代码到本地仓库
- **小功能粒度提交**：每完成一个小功能提交一次本地 git 仓库，提交信息概括该功能的完成情况
- 实施里程碑 P0–P3：桌面端随 P0/P1 同步实现，安卓端 P1 完成后跟进（P3 商业化暂缓，按用户指示暂停）
- 领域模型要点（限界上下文、聚合规则）见 architecture.md 第六章
