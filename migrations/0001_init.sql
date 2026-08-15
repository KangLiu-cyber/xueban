-- 学伴数据库初始结构（第一版正式库）：与 docs/architecture.md §7 数据模型一致
-- 删除/更新级联（原 0002）：删除目录/笔记时级联清理子树、批注与归属习题，
-- 习题删除进一步级联作答记录与错题；events.item_id 无外键，保留历史快照。

-- 身份与认证
create table users (
  id            bigserial primary key,
  account       text unique not null,      -- 手机号/邮箱/用户名
  password_hash text not null,             -- argon2id
  nickname      text,
  created_at    timestamptz default now()
);

create table tokens (
  id         bigserial primary key,
  user_id    bigint not null references users,
  token      text unique not null,         -- usr_ 前缀
  purpose    text not null,                -- client | agent
  revoked_at timestamptz,
  created_at timestamptz default now()
);

create index tokens_user_id_idx on tokens (user_id);

-- 学习空间
create table workspaces (
  id         bigserial primary key,
  user_id    bigint not null references users,
  name       text not null,
  exam_goal  text not null,                -- 手写考试目标
  exam_date  date,
  created_at timestamptz default now()
);

create index workspaces_user_id_idx on workspaces (user_id);

create table items (
  id           bigserial primary key,
  workspace_id bigint not null references workspaces,
  parent_id    bigint references items on delete cascade,  -- 空为根节点，自由嵌套；级联子树
  kind         text not null,              -- dir | note
  name         text not null,
  content      text,                       -- note 的 Markdown 正文
  created_by   text not null,              -- agent | user
  created_at   timestamptz default now(),
  updated_at   timestamptz default now()
);

create index items_workspace_parent_idx on items (workspace_id, parent_id);

create table annotations (
  id         bigserial primary key,
  item_id    bigint not null references items on delete cascade,
  user_id    bigint not null references users,
  author     text not null,                -- ai | user
  anchor     text not null,                -- 正文引用片段（定位用）
  text       text not null,
  created_at timestamptz default now()
);

create index annotations_item_id_idx on annotations (item_id);

-- 练习
create table questions (
  id             bigserial primary key,
  workspace_id   bigint not null references workspaces,
  source_item_id bigint not null references items on delete cascade,  -- 归属的"集"；内容删除级联其题库
  type           text not null,            -- single | multi | judge
  stem           text not null,
  options        jsonb,
  answer         jsonb not null,
  explanation    text,
  created_at     timestamptz default now()
);

create index questions_workspace_source_idx on questions (workspace_id, source_item_id);

create table quiz_records (
  id          bigserial primary key,
  user_id     bigint not null references users,
  question_id bigint not null references questions on delete cascade,
  scope       text,                        -- 刷题范围快照
  chosen      jsonb,
  is_correct  boolean not null,
  created_at  timestamptz default now()
);

create index quiz_records_user_created_idx on quiz_records (user_id, created_at);

create table wrong_items (
  id          bigserial primary key,
  user_id     bigint not null references users,
  question_id bigint not null references questions on delete cascade,
  times       int not null default 1,
  mastered    boolean not null default false,
  updated_at  timestamptz default now(),
  unique (user_id, question_id)
);

create table papers (
  id           bigserial primary key,
  user_id      bigint not null references users,
  workspace_id bigint not null references workspaces,
  name         text,
  config       jsonb not null,             -- 来源/题型/范围/数量
  question_ids jsonb not null,             -- 抽题快照
  result       jsonb,                      -- 交卷后：得分/正确率/用时
  created_at   timestamptz default now()
);

create index papers_user_id_idx on papers (user_id);

-- 用户自定义 Skill：用户保存自己的 skill（名称 + 介绍 + 脚本内容），
-- 随 bootstrap 能力下发与系统内置 Skill 目录合并（同名用户覆盖内置），
-- Agent 可按名经 get_skill 拉取完整内容。
-- 与内置 Skill 目录（skills/ 文件夹文件资产，无表）区分：本表按用户隔离。
create table skills (
  id          bigserial primary key,
  user_id     bigint not null references users,
  name        text not null,
  description text not null,
  script      text,                          -- 可空：纯文字说明型 skill 无脚本
  created_at  timestamptz default now(),
  unique (user_id, name)
);

create index skills_user_id_idx on skills (user_id);

-- 协作事件
create table events (
  id           bigserial primary key,
  user_id      bigint not null references users,
  workspace_id bigint,
  item_id      bigint,
  action       text not null,              -- annotate/answer/wrong/agent_write...
  payload      jsonb,
  created_at   timestamptz default now()
);

create index events_user_created_idx on events (user_id, created_at);
