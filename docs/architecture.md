# 超级学习助手 · 系统架构设计

版本：v1.1 ｜ 日期：2026-08-15 ｜ 状态：评审稿
关联文档：《requirements.md》（需求 v7.2）、桌面端原型 v3 柔和版、安卓端原型 v1

v1.1 变更：后端改为六边形架构（端口与适配器）+ DDD，新增限界上下文划分、端口定义与 Rust 工程结构。

## 一、设计目标与约束

系统要解决的核心问题：备考用户把课程视频交给 AI，得到结构化的笔记与习题，然后在同一个空间里完成刷题、错题归集、组卷模考与复盘。AI 生成能力不在我们服务器上运行，而是由用户自己的 Agent（如 TRAE）装载我们下发的 Skill 与提示词后执行，我们的服务负责数据存储、用户体系与协作接口。

由此得出五条硬约束，贯穿全部架构决策：

1. 服务端不做 LLM 推理。AI 生成（目录、笔记、批注、习题）发生在 Agent 侧，服务端只做校验与存储，服务器成本与用户量近似线性于存储和带宽，不受推理成本牵制。
2. 数据按用户严格隔离。Agent 持有的是某个用户的专属凭证，只能读写该用户的数据，任何接口不得跨用户返回内容。
3. 内容结构自由嵌套。目录、文件（笔记）由 AI 生成，层级不固定，数据模型必须支持任意深度的树形结构，而不是写死"课程—集数"两级。
4. 成本红线。单用户日均处理成本不超过 0.5 元（该成本主要发生在 Agent 侧的生成环节），服务端轻量部署即可满足。
5. 业务规则与技术设施解耦。后端采用六边形架构与依赖倒置，领域层不依赖任何框架与数据库，客户端 API、MCP 协议、存储实现都可独立替换。

客户端范围：桌面端（Tauri）与安卓端（含折叠屏），两端共用同一套后端 API。原型均已定稿，见 ui-mockup 目录。

## 二、总体架构

```
┌────────────────────────────┐        ┌────────────────────────────┐
│  客户端                      │        │  Agent 生态（AI 生成侧）      │
│  桌面 Tauri / 安卓 Compose    │        │  TRAE / 任意支持 MCP 的 Agent │
└───────┬────────────────────┘        └────┬───────────────────────┘
        │ HTTPS（账号登录 / token）            │ MCP over HTTPS（用户 token）
        ▼                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│  驱动适配器（左侧入口）                                              │
│  ┌──────────────────────┐   ┌─────────────────────────────────┐ │
│  │ REST 适配器（Axum）     │   │ MCP 适配器（网关 + 能力下发）       │ │
│  │ 认证 / 限流 / 审计       │   │ token 校验 → 绑定用户上下文        │ │
│  └──────────┬───────────┘   └───────────────┬─────────────────┘ │
│             └──────────┬─────────────────────┘                   │
│                        ▼ 实现输入端口（用例）                          │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ 应用层（用例编排）                                               │ │
│  │ 注册登录 / 空间与内容管理 / 刷题 / 错题本 / 组卷模考 / Agent 装配    │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│                             ▼ 只依赖领域层                           │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ 领域层（纯业务规则，零框架依赖）                                    │ │
│  │ User/Token · Workspace/Item/Annotation · Question/Wrong/Paper │ │
│  │ 仓储端口（traits）· 领域服务 · 业务不变式                           │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│                             ▲ 实现输出端口（仓储/事件存储）             │
│  ┌──────────────────────────┴──────────────────────────────────┐ │
│  │ 被驱动适配器（右侧出口）                                          │ │
│  │ Postgres（SQLx）· Redis（可选）· 对象存储（可选）                  │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

分层说明：

- 客户端只与 REST 适配器通信，不接触 LLM，不直连 MCP。桌面端与安卓端逻辑一致：登录取 token，之后所有请求携带 token。
- Agent 只与 MCP 适配器通信。用户把接入凭证（MCP 端点 + 用户 token）粘贴给 Agent，Agent 连接后由能力下发用例返回 Skill、提示词与工具清单，随后 Agent 以该用户身份调用 MCP 工具读写数据。
- 后端是单进程部署的六边形架构应用：领域层在中心，应用层编排用例，REST 与 MCP 是两个驱动适配器，Postgres 等是被驱动适配器。依赖方向永远指向内层，领域层不知道 HTTP、MCP、SQL 的存在。
- REST 与 MCP 两个入口复用同一组用例：客户端刷题和 Agent 写入习题走的是同一条业务规则路径，不会出现两套逻辑分叉。

## 三、关键架构决策

| 决策 | 选择 | 理由 |
|:---|:---|:---|
| AI 生成的执行位置 | Agent 侧（用户的 Agent 执行我们下发的 Skill） | 服务端零推理成本；生成质量随 Agent 能力升级而提升；与成本红线兼容 |
| 后端架构风格 | 六边形架构（端口与适配器）+ 依赖倒置 | 业务规则集中在领域层，协议与存储可替换；REST 与 MCP 复用同一套用例 |
| 领域建模方法 | DDD：战略上划分限界上下文，战术上用聚合/实体/值对象建模 | 内容树、题库、错题本各有独立不变式，聚合边界清晰后隔离与测试都简单 |
| 服务端技术栈 | Rust（Axum）+ PostgreSQL | 与既定选型一致；单二进制部署，内存占用低，适合小规模起步 |
| 部署形态 | 单进程（模块化 crate workspace），不拆微服务 | 团队规模小；六边形边界已提供拆分线，未来可按上下文拆出独立服务 |
| Agent 接入协议 | MCP 作为唯一接入方式 | 主流 Agent 均已支持；工具清单即能力边界，天然可控可审计 |
| 认证方式 | 账号密码注册登录 + 每用户独立 token | 订阅计费与数据归属都依赖账号体系；token 是 Agent 与客户端的统一身份载体 |
| 内容模型 | 通用树（items 表 parent_id 自由嵌套） | 目录结构由 AI 生成，层级不可预设；树模型同时容纳目录与笔记 |
| 题库归属 | 题目挂在"集"（笔记 item）之下 | 刷题范围、组卷筛选都按集归属；与"刷题数据来自每集 AI 生成习题"的产品定义一致 |
| 客户端形态 | 桌面 Tauri、安卓 Kotlin + Compose | 桌面先行验证闭环，安卓按已定稿原型跟进；两端共用后端 |

## 四、DDD 战略设计：限界上下文

系统划分为四个限界上下文，每个上下文有独立的聚合与业务语言，上下文之间通过应用层用例协作，不直接跨上下文访问对方聚合内部：

| 限界上下文 | 核心聚合 | 职责 |
|:---|:---|:---|
| 身份与认证 | User（根）、Token | 注册、登录、凭证签发与吊销、密码策略 |
| 学习空间 | Workspace（根）、Item、Annotation | 考试目标与日期、内容树（目录/笔记）、批注规则 |
| 练习 | Question、WrongItem（根）、Paper（根）、QuizRecord | 题库归属、作答判定、错题归集与掌握、组卷抽题与模考判分 |
| 协作事件 | Event | 行为追加记录，供复盘 Agent 读取与审计 |

上下文映射关系：

- 身份与认证 → 其他上下文：所有用例从请求上下文取得 user_id（由认证上下文签发的 token 解析而来），作为查询与写入的强制边界。
- 学习空间 → 练习：题目通过 `source_item_id` 引用某一集（Item 的 id），这是两个上下文唯一的集成点；Item 删除时级联处理归属题目。
- 练习 → 协作事件：作答、错题、组卷等关键动作产生领域事件，由应用层统一追加到事件上下文。
- Agent 写入（MCP 适配器）→ 学习空间 / 练习：Agent 的写入与客户端操作落在同一组用例上，仅创建者标记不同（created_by = agent）。

## 五、六边形架构与依赖倒置

### 5.1 三层与依赖规则

- 领域层：实体、值对象、领域服务、业务不变式，以及仓储端口的 trait 定义。只依赖标准库和少量纯函数库，禁止出现 tokio、axum、sqlx、serde_json 之外的框架类型。
- 应用层：用例（输入端口的实现）与输出端口定义。用例负责编排：取聚合 → 调领域逻辑 → 经端口持久化 → 发领域事件。不含 SQL，不含 HTTP 概念。
- 适配器层：驱动适配器把外部协议翻译成用例调用（REST handler、MCP tool handler）；被驱动适配器实现输出端口（Postgres 仓储、事件存储）。

依赖规则：外层依赖内层，内层不知道外层存在。领域层定义"我需要什么"（trait），基础设施层实现"怎么提供"，由 bootstrap 在启动时注入。

### 5.2 端口清单

输入端口（应用层用例，驱动适配器调用）：

| 用例 | 驱动入口 |
|:---|:---|
| Register / Login / Logout | REST |
| ManageExamGoal（改目标与日期） | REST |
| BrowseTree / ReadNote / Annotate | REST + MCP |
| SaveQuestions（Agent 写入习题） | MCP |
| DrawQuestions / SubmitAnswer（刷题） | REST |
| ListWrong / RedoWrong / MarkMastered | REST |
| AssemblePaper / SubmitPaper（组卷模考） | REST |
| AgentBootstrap（能力下发） | MCP |
| ReadEvents / ReportStatus | MCP |

输出端口（领域/应用层定义 trait，被驱动适配器实现）：

| 端口 | 说明 | Postgres 适配器实现要点 |
|:---|:---|:---|
| UserRepository / TokenRepository | 账号与凭证存取 | argon2 哈希、token 唯一索引 |
| WorkspaceRepository | 空间与考试目标 | exam_goal 手写文本 |
| ItemRepository | 内容树读写 | parent_id 递归查询（WITH RECURSIVE） |
| AnnotationRepository | 批注存取 | author 区分 ai/user |
| QuestionRepository | 题库读写 | 按 workspace + source_item 查询 |
| QuizRecordRepository | 作答记录追加 | 只追加 |
| WrongItemRepository | 错题聚合存取 | (user_id, question_id) 唯一，times 累加 |
| PaperRepository | 试卷快照存取 | question_ids 用 JSONB |
| EventStore | 事件追加与按用户回放 | 按 (user_id, created_at) 索引 |
| CredentialIssuer | token 生成 | 随机 32 字节 Base62，usr_ 前缀 |

### 5.3 依赖倒置示例：刷题用例

以"提交一道题的答案"说明依赖方向：

1. 领域层：`Question` 实体提供 `judge(chosen) -> Judgment` 纯函数；`WrongItem` 聚合定义不变式——答错则 `times += 1` 且 `mastered = false`，重做答对不自动清除错题（由用户显式标记掌握）。
2. 应用层：`SubmitAnswer` 用例依赖 `QuestionRepository`、`QuizRecordRepository`、`WrongItemRepository`、`EventStore` 四个 trait，编排"取题 → 判定 → 记错题 → 追加事件"。
3. 被驱动适配器：`adapter-postgres` 用 SQLx 实现上述 trait。
4. 驱动适配器：REST 的 `POST /api/v1/quiz/answer` 把请求体映射为用例入参。若未来 Agent 也要判题，MCP 适配器调用同一个用例即可，规则零重复。

### 5.4 工程结构

```
├── Cargo.toml                   # 后端 workspace 定义（members = crates/*，exclude = clients）
├── crates/                      # 后端：六边形架构 + DDD
│   ├── domain/                  # 领域层：零框架依赖
│   │   └── src/
│   │       ├── identity.rs      # User、Token、密码与凭证规则
│   │       ├── space.rs         # Workspace、Item 树、Annotation
│   │       ├── practice.rs      # Question、WrongItem、Paper、抽题/判分领域服务
│   │       ├── skill.rs         # Skill（内置目录解析 skills/*.md）
│   │       ├── event.rs         # Event、领域事件定义
│   │       └── ports.rs         # 仓储端口 traits（输出端口）
│   ├── application/             # 应用层：用例编排
│   │   └── src/
│   │       ├── auth.rs          # Register / Login / Logout
│   │       ├── space.rs         # ManageExamGoal / BrowseTree / Annotate
│   │       ├── quiz.rs          # DrawQuestions / SubmitAnswer
│   │       ├── wrong.rs         # ListWrong / RedoWrong / MarkMastered
│   │       ├── paper.rs         # AssemblePaper / SubmitPaper
│   │       └── agent.rs         # AgentBootstrap / ReadEvents / ReportStatus / GetSkill
│   ├── adapter-http/            # 驱动适配器：Axum REST API（/api/v1）
│   │   └── src/
│   │       ├── lib.rs           # 路由组装与中间件
│   │       ├── agent.rs         # Agent 凭证端点
│   │       ├── auth.rs          # 注册 / 登录 / 注销
│   │       ├── middleware.rs    # require_auth + 限流
│   │       └── ...              # space / quiz / wrong / paper 等端点
│   ├── adapter-mcp/             # 驱动适配器：MCP 网关 + 能力下发
│   ├── adapter-postgres/        # 被驱动适配器：SQLx 仓储与事件存储实现
│   │   └── src/
│   │       ├── lib.rs           # map_sqlx_error 与导出
│   │       └── ...              # 其余 Pg*Repository 与 PgEventStore
│   └── bootstrap/               # 组装：依赖注入、配置、迁移、main
├── clients/                     # 客户端：独立 workspace（被后端 exclude）
│   ├── Cargo.toml               # 前端 workspace 定义（members = web, desktop）
│   ├── web/                     # Leptos 应用（lib crate，编译 WASM）
│   │   └── src/
│   │       ├── lib.rs           # 应用根组件与路由
│   │       ├── api.rs           # 后端 /api/v1 客户端封装
│   │       ├── pages/           # 页面：登录、空间树、刷题、错题本、组卷、复盘
│   │       └── components/      # 通用组件
│   ├── desktop/                 # Tauri 壳（bin crate，依赖 web）
│   │   └── src/main.rs          # tauri::Builder 启动，加载 web 构建产物
│   └── android/                 # 安卓端：Kotlin + Jetpack Compose
│       └── app/                 # 主应用模块（Material 3，WindowSizeClass 折叠屏适配）
├── migrations/                  # SQL 迁移脚本
└── docs/                        # 架构 / 需求文档与 UI 原型
```

依赖方向：`bootstrap → adapter-* → application → domain`；`domain` 不依赖 workspace 内任何其他 crate。CI 中用 cargo deny / 依赖检查守住 domain 的纯净边界。领域层「零框架依赖」指不引入运行时框架；`async-trait` 等纯语法宏（仅把 async fn 展开为 boxed future，换取端口 dyn 兼容性，供 bootstrap 以 `Arc<dyn Trait>` 组装注入）允许。

前端：UI 代码只有一份（`web`），编译 WASM 供浏览器直接运行，Tauri 壳加载同一构建产物，`desktop → web` 建立链接；`clients/` 独立 workspace，避免 wasm target 与 Tauri 原生依赖拖累后端编译循环。安卓端（Kotlin）与 Rust 前端独立，仅共用后端 API。

## 六、领域模型要点

### 6.1 身份与认证上下文

- 密码使用 argon2id 哈希存储，不存明文；密码强度校验在领域层（值对象 `Password`）。
- 登录成功签发长期 token，同一用户可存在多个 token（客户端一个、Agent 一个），支持单独吊销。Token 是实体，`purpose` 区分 client / agent。
- 考试目标（手写文本）与考试日期属于 Workspace 聚合，注册后第二步填写。

### 6.2 学习空间上下文

- Workspace 聚合根保存 `exam_goal`（手写文本，不做枚举）与 `exam_date`，客户端据此渲染倒计时。
- Item 分两类：`dir`（目录）与 `note`（笔记），通过 parent_id 任意嵌套。树的不变式在领域服务中维护：节点不可挂到自身子树下（防环）。
- Annotation 区分作者：`ai`（Agent 写入的老师强调、考点提示）与 `user`（用户手写）。AI 批注不可被用户修改，只能删除；用户批注可编辑删除。锚点以正文引用片段定位。
- 所有写入校验 `workspace.user_id == 当前用户`，user_id 来自 token 解析的上下文，不接受客户端声明。

### 6.3 练习上下文

- Question 必须关联某个笔记 Item（即"集"），这是刷题范围与组卷筛选的唯一归属维度。题目值对象含题型、题干、选项、答案、解析。
- 作答判定是领域纯函数：`Question::judge` 返回对错，应用层据此追加 QuizRecord 并更新 WrongItem。
- WrongItem 聚合：同一题错多次记 `times`，用户可标记 `mastered`；重做只针对单题，不产生新的刷题会话。
- Paper 聚合：组卷是"筛选条件 + 题目快照"，抽题是领域服务（来源、题型、范围、数量约束，不足时按规则补齐）；交卷判分后写结果并重算错题本。

### 6.4 协作事件上下文

- Event 只追加不修改，记录 annotate、answer、wrong、agent_write 等行为，按 user_id 查询。
- 复盘闭环的数据基础：Agent 通过 ReadEvents 拿到最近的错题与答题行为，生成复盘内容写回学习空间。

## 七、数据模型

持久化由 adapter-postgres 实现，表结构与聚合对应：

```sql
-- 身份与认证
users (
  id            bigserial primary key,
  account       text unique not null,      -- 手机号/邮箱/用户名
  password_hash text not null,             -- argon2id
  nickname      text,
  created_at    timestamptz default now()
)

tokens (
  id         bigserial primary key,
  user_id    bigint not null references users,
  token      text unique not null,         -- usr_ 前缀
  purpose    text not null,                -- client | agent
  revoked_at timestamptz,
  created_at timestamptz default now()
)

-- 学习空间
workspaces (
  id        bigserial primary key,
  user_id   bigint not null references users,
  name      text not null,
  exam_goal text not null,                 -- 手写考试目标
  exam_date date,
  created_at timestamptz default now()
)

items (
  id           bigserial primary key,
  workspace_id bigint not null references workspaces,
  parent_id    bigint references items,    -- 空为根节点，自由嵌套
  kind         text not null,              -- dir | note
  name         text not null,
  content      text,                       -- note 的 Markdown 正文
  created_by   text not null,              -- agent | user
  created_at   timestamptz default now(),
  updated_at   timestamptz default now()
)

annotations (
  id         bigserial primary key,
  item_id    bigint not null references items,
  user_id    bigint not null references users,
  author     text not null,                -- ai | user
  anchor     text not null,                -- 正文引用片段（定位用）
  text       text not null,
  created_at timestamptz default now()
)

-- 练习
questions (
  id             bigserial primary key,
  workspace_id   bigint not null references workspaces,
  source_item_id bigint not null references items,  -- 归属的"集"
  type           text not null,            -- single | multi | judge
  stem           text not null,
  options        jsonb,
  answer         jsonb not null,
  explanation    text,
  created_at     timestamptz default now()
)

quiz_records (
  id          bigserial primary key,
  user_id     bigint not null references users,
  question_id bigint not null references questions,
  scope       text,                        -- 刷题范围快照
  chosen      jsonb,
  is_correct  boolean not null,
  created_at  timestamptz default now()
)

wrong_items (
  id          bigserial primary key,
  user_id     bigint not null references users,
  question_id bigint not null references questions,
  times       int not null default 1,
  mastered    boolean not null default false,
  updated_at  timestamptz default now(),
  unique (user_id, question_id)
)

papers (
  id           bigserial primary key,
  user_id      bigint not null references users,
  workspace_id bigint not null references workspaces,
  name         text,
  config       jsonb not null,             -- 来源/题型/范围/数量
  question_ids jsonb not null,             -- 抽题快照
  result       jsonb,                      -- 交卷后：得分/正确率/用时
  created_at   timestamptz default now()
)

-- 协作事件
events (
  id           bigserial primary key,
  user_id      bigint not null references users,
  workspace_id bigint,
  item_id      bigint,
  action       text not null,              -- annotate/answer/wrong/agent_write...
  payload      jsonb,
  created_at   timestamptz default now()
)
```

索引要点：`items(workspace_id, parent_id)`、`questions(workspace_id, source_item_id)`、`quiz_records(user_id, created_at)`、`events(user_id, created_at)`。所有表的查询必须携带 user_id 条件（workspaces 以下通过 workspace 归属间接限定）。仓储实现统一在 SQL 层强制该条件，作为隔离的第二道防线。

## 八、接口设计（驱动适配器）

### 8.1 REST 适配器（客户端）

认证头统一为 `Authorization: Bearer <token>`。handler 只做参数映射与错误翻译，业务逻辑全部委托用例。

| 分组 | 接口 | 对应用例 |
|:---|:---|:---|
| 认证 | `POST /api/v1/auth/register` | Register |
| 认证 | `POST /api/v1/auth/login` | Login |
| 认证 | `POST /api/v1/auth/logout` | Logout |
| 空间 | `GET /api/v1/workspaces` | BrowseTree（空间列表） |
| 空间 | `POST /api/v1/workspaces` | ManageExamGoal（创建空间） |
| 空间 | `PUT /api/v1/workspaces/:id` | ManageExamGoal（更新目标/日期） |
| 内容 | `GET /api/v1/workspaces/:id/tree` | BrowseTree |
| 内容 | `GET /api/v1/items/:id` | ReadNote |
| 内容 | `DELETE /api/v1/items/:id` | ManageContent（删除，级联子树/批注/归属题目） |
| 批注 | `POST /api/v1/items/:id/annotations` | Annotate |
| 批注 | `PUT /api/v1/annotations/:id` | Annotate（编辑文本，仅我的批注；AI 批注拒绝） |
| 批注 | `DELETE /api/v1/annotations/:id` | Annotate（删除） |
| 刷题 | `GET /api/v1/quiz/questions?scope=` | DrawQuestions |
| 刷题 | `POST /api/v1/quiz/answer` | SubmitAnswer |
| 错题 | `GET /api/v1/wrong` | ListWrong |
| 错题 | `GET /api/v1/wrong/stats` | WrongStats（累计/近 7 天新增/已掌握，错题本统计卡片） |
| 错题 | `POST /api/v1/wrong/:id/master` | MarkMastered |
| 错题 | `POST /api/v1/wrong/:id/unmaster` | MarkMastered（取消掌握） |
| 组卷 | `POST /api/v1/papers` | AssemblePaper |
| 组卷 | `GET /api/v1/papers/:id` | ReadPaper |
| 组卷 | `POST /api/v1/papers/:id/submit` | SubmitPaper |
| Agent | `GET /api/v1/agent/credential` | 读取接入凭证文案 |
| Agent | `POST /api/v1/agent/credential/rotate` | 换发 token |

作答与答案的 JSON 格式约定（`questions.answer`、`quiz_records.chosen`、REST 请求/响应共用）：

- `single`：选项索引数字，如 `1`；`multi`：索引数组，如 `[0, 2]`；`judge`：布尔，如 `true`。
- 抽题响应（DrawQuestions）不携带答案与解析，防客户端作弊；提交作答后服务端返回判定、正确答案与解析。

### 8.2 MCP 适配器（Agent 侧）

MCP tool handler 与 REST handler 一样只做协议翻译，复用同一组用例。连接建立时校验 token 并绑定用户上下文，工具入参中不接受用户身份字段。

| 工具 | 对应用例 |
|:---|:---|
| `bootstrap` | AgentBootstrap（返回备考提示词、工具清单、内置 Skill 目录全量内容 `skills`（含脚本）） |
| `create_workspace` | ManageExamGoal（创建） |
| `create_item` / `write_item` | BrowseTree / ReadNote 的写入侧（Agent 内容写入） |
| `read_item` / `list_items` | BrowseTree / ReadNote |
| `add_annotation` | Annotate（author = ai） |
| `save_questions` | SaveQuestions |
| `get_events` | ReadEvents |
| `report_status` | ReportStatus |
| `get_skill` | GetSkill（按名拉取内置 skill 完整内容，含脚本，重新安装/更新用） |

MCP 适配器的强制规则：

- 每个请求先校验 token，解析出 user_id 注入用例上下文。
- Agent 写入全部记入 events（action = agent_write），客户端可展示"由 AI 生成"的来源标注。
- 单 token 限流（默认 60 次/分钟），防止异常 Agent 打满服务。
- 能力包（Skill + 提示词 + 工具清单 + 内置 Skill 目录）按版本管理，服务端升级后 Agent 下次接入自动获取新版本，无需用户重新复制凭证。内置 Skill 目录为全局共享资产：开发者把 skill 安装包（一个 `.md` 文件一个 skill，含名称、介绍与脚本）放进仓库根 `skills/` 文件夹，后端启动时自动加载解析（见 §5.4 `domain/src/skill.rs`）。`bootstrap` 全量下发全部 skill（含脚本），Agent 首次接入即自动下载安装；之后按名调 `get_skill` 重新拉取（更新/修复安装）。所有用户接入拿到同一份清单，无用户维度区分。

## 九、核心流程

### 9.1 注册登录与备考空间初始化

```
用户            客户端                 REST 适配器 → 用例 → 仓储
 │  注册(账号/密码)  │                     │
 │───────────────▶│── POST /auth/register ▶│ Register：建 User，密码 argon2 哈希
 │  登录           │                     │
 │───────────────▶│── POST /auth/login ──▶│ Login：签发 Token(purpose=client)
 │  手写考试目标+日期 │                     │
 │───────────────▶│── PUT /workspaces ──▶│ ManageExamGoal：建/改 Workspace
 │  进入系统，倒计时开始│◀── 返回空间信息 ──────│
 │  弹出 Agent 接入凭证│── GET /agent/credential ▶│ 返回凭证文案(含 token)
```

### 9.2 Agent 接入与内容生成

```
用户                任意 Agent              MCP 适配器 → 用例 → 仓储
 │ 复制凭证(MCP端点+token) │                        │
 │─────────────────────▶│                        │
 │                      │── MCP 连接(token) ─────▶│ 校验 token → 绑定 user_id
 │                      │◀── bootstrap ──────────│ AgentBootstrap：Skill+提示词+工具清单
 │                      │   （Agent 装配能力）        │
 │ "帮我学第3集"           │                        │
 │─────────────────────▶│── read_item/list_items ─▶│ BrowseTree/ReadNote：查重
 │                      │   （Agent 侧执行生成）       │
 │                      │── create_item/write_item ▶│ 写入笔记（记 agent_write 事件）
 │                      │── add_annotation ───────▶│ Annotate(author=ai)
 │                      │── save_questions ───────▶│ SaveQuestions：写入该集习题
 │                      │── report_status ────────▶│ 客户端刷新生成状态
```

### 9.3 内容数据流向

AI 生成是唯一的原始数据源，刷题、错题本、组卷都是使用过程中的派生数据：

```
Agent 生成（原始数据）                使用过程（派生数据）
┌─────────────────────┐          ┌──────────────────────────┐
│ 目录结构(items/dir)   │          │ 刷题：从题库按集取题           │
│ 笔记(items/note)     │──题目归属──▶│   ↓ 答错                   │
│ AI 批注(annotations) │          │ 错题本：自动归集，可重做/标掌握    │
│ 习题(questions)      │          │   ↓ 作为组卷来源之一            │
└─────────────────────┘          │ 组卷：筛选抽题 → 模考 → 判分     │
                                 │   ↓ 答错回流错题本              │
                                 │ 复盘：Agent 读 events 写回笔记   │
                                 └──────────────────────────┘
```

## 十、安全与数据隔离

- 身份：密码 argon2id；token 随机 32 字节，泄露时可单独吊销并重发（凭证换发后旧 token 立即失效）。
- 隔离：数据归属链为 token → user → workspace → items/questions/...。用例上下文统一携带 user_id，仓储实现强制在 SQL 中附加该条件；MCP 工具入参中不存在用户身份字段，从协议层杜绝串用户。
- 传输：全站 HTTPS；MCP 通道同样走 HTTPS。
- 审计：Agent 的每次写入、用户的每次作答都落 events 表，可按用户回溯全部内容变更。
- 输入校验：驱动适配器做协议层校验（格式、大小），领域层做业务校验（归属、不变式）。笔记正文与题目写入有大小上限（单 item 正文 ≤ 512KB，单批题目 ≤ 200 题）。
- 限流：REST 与 MCP 均按 token 限流，REST 鉴权通过后另按账号（user_id）封顶，防多 token 叠加绕限；注册接口按 IP 限流防刷。计数为进程内存固定窗口（单机部署足够，Redis 二期可选引入，见 §9）。

## 十一、部署架构

起步阶段单机部署，Docker Compose 编排：

```
┌────────────────────────────────────────────┐
│  单台云服务器（2C4G 起步）                      │
│  ┌──────┐   ┌──────────────────────────┐   │
│  │ Caddy │──▶│ app（Rust 单进程）          │   │
│  │ 反代+  │   │  REST 适配器 + MCP 适配器   │   │
│  │ 自动证书│   └───────────┬──────────────┘   │
│  └──────┘               ▼                  │
│              ┌──────────────┐  ┌─────────┐ │
│              │ PostgreSQL 16 │  │ Redis(可选)│ │
│              └──────────────┘  └─────────┘ │
│  每日 pg_dump → 对象存储，保留 30 天            │
└────────────────────────────────────────────┘
```

- Caddy 负责 TLS 与域名反代，自动续期证书。
- 服务端无推理负载，2C4G 可支撑数千日活；瓶颈出现时优先升配。
- 演进路径：单进程 → 读写分离（Postgres 只读副本）→ 按限界上下文拆服务（六边形边界即拆分线，仅当团队与流量同时增长时）。

## 十二、非功能设计

| 维度 | 指标与做法 |
|:---|:---|
| 性能 | 内容树接口 P95 ≤ 300ms（单空间 ≤ 2000 节点）；刷题取题 P95 ≤ 200ms；客户端首屏先拉树、正文懒加载 |
| 可测试性 | 领域层纯函数，单元测试不依赖数据库；用例层用内存仓储（mock 端口）测试；适配器层做集成测试（testcontainers 起 Postgres） |
| 可用性 | 目标 99.5%（备考工具可容忍短暂不可用）；数据库每日备份 + WAL 归档 |
| 可观测 | 结构化日志（tracing）+ /healthz 探针；events 表兼作用户行为分析源 |
| 成本控制 | 服务端零 LLM 调用；单用户服务端成本 ≈ 存储 + 带宽，远低于 0.5 元/日红线；生成成本在 Agent 侧，由 Skill 内的缓存与本地化策略约束 |
| 兼容性 | 桌面端与安卓端共用 REST API 版本（/api/v1）；API 变更走新增字段不删字段 |

## 十三、技术选型清单

| 层 | 选型 | 备注 |
|:---|:---|:---|
| 后端架构 | 六边形架构 + DDD | crate workspace 分层，依赖指向领域层 |
| 服务端语言 | Rust + Axum + SQLx | 单二进制，domain crate 零框架依赖 |
| 数据库 | PostgreSQL 16 | 树结构 + JSONB 题目/事件 |
| 缓存 | Redis（可选，二期引入） | 会话缓存、限流计数 |
| 桌面端 | Tauri + Web 前端 | 按 v3 柔和版原型实现 |
| 安卓端 | Kotlin + Jetpack Compose + Material 3 | 按安卓原型 v1 实现；WindowSizeClass 处理折叠屏（≥600dp 双栏） |
| Agent 接入 | MCP over HTTPS | 能力包 = Skill + 提示词 + 工具清单 |
| 部署 | Docker Compose + Caddy | 单机起步 |

## 十四、实施里程碑

| 阶段 | 范围 | 验收标准 |
|:---|:---|:---|
| P0 账号与接入 | 六边形工程骨架（crate 分层 + CI 依赖检查）、注册 / 登录 / token、工作空间与考试目标、MCP 适配器 + 能力下发、items 树读写 | Agent 复制凭证接入后能生成目录与笔记，客户端可见；两用户数据互不可见；domain crate 无框架依赖 |
| P1 学习闭环 | 批注、题库写入、刷题、错题本 | 笔记可批注；每集习题可刷；答错自动进错题本并可重做；用例层测试全绿 |
| P2 模考与复盘 | 组卷、模考判分、复盘事件读取、学习统计 | 组卷 75 题模考全流程跑通；复盘 Agent 能读到错题事件 |
| P3 商业化基础 | 订阅计费接口、配额管理 | 账号体系对接支付，按订阅控制配额 |

客户端节奏：桌面端随 P0/P1 同步实现，安卓端在 P1 完成后按已定稿原型开发，两端共用同一后端，不重复建设。
