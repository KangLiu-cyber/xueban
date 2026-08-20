-- 无感登录 / 滑动会话：为 tokens 增加活跃时间与过期时间。
--
-- last_used_at：最近一次鉴权通过的时间（记录登录/活跃时间）。
-- expires_at：过期时间。client token 签发时设为 now + 30 天，之后每次
--   鉴权通过向后滑动（滑动窗口）；agent token 保持 NULL（永不过期，仅可吊销）。
--   已存在的行两列均为 NULL：表示永不过期、无活跃记录，不强制登出存量用户。
alter table tokens
  add column last_used_at timestamptz,
  add column expires_at   timestamptz;
