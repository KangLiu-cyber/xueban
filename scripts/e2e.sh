#!/usr/bin/env bash
# 企业场景端到端冒烟测试：真实服务进程 + 真实 PostgreSQL + 真实 HTTP。
# 覆盖：启动冒烟 → 认证与鉴权 → 体育学习闭环（内容→刷题→错题→打卡）→
# 参数校验 → 跨用户隔离 → 事件流落库 → 服务重启后数据持久。
#
# 用法：scripts/e2e.sh [DATABASE_URL]（默认 make dev 同款连接串）
# 前置：本机 PostgreSQL 已启动；端口 8080 空闲（BIND_ADDR 可覆盖）。
# 退出码：0 全绿，1 有失败。测试数据用时间戳账户，不清理（与集成测试同策略）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATABASE_URL="${1:-${DATABASE_URL:-postgres://postgres@localhost/xueban?host=/tmp}}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:8080}"
BASE="http://${BIND_ADDR}/api/v1"

# psql 定位：PATH 优先，兜底 macOS Postgres.app 内置客户端
PSQL="${PSQL:-$(command -v psql || true)}"
if [ -z "${PSQL}" ]; then
  PSQL="$(ls /Applications/Postgres.app/Contents/Versions/*/bin/psql 2>/dev/null | head -1 || true)"
fi
if [ -z "${PSQL}" ]; then
  echo "找不到 psql：请安装 PostgreSQL 客户端或设置 PSQL 环境变量" >&2
  exit 1
fi

BODY="$(mktemp -t xueban-e2e-body.XXXXXX)"
LOG="$(mktemp -t xueban-e2e-server.XXXXXX.log)"
SRV_PID=""
PASS=0
FAIL=0

cleanup() {
  if [ -n "${SRV_PID}" ]; then
    kill "${SRV_PID}" 2>/dev/null || true
    wait "${SRV_PID}" 2>/dev/null || true
  fi
  rm -f "$BODY"
  [ "${FAIL}" -eq 0 ] && rm -f "$LOG" || true
}

trap cleanup EXIT

say() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }
ok()  { PASS=$((PASS + 1)); printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad() { FAIL=$((FAIL + 1)); printf '  \033[31m✗ %s\033[0m\n' "$*"; }

# req METHOD PATH [TOKEN] [BODY] —— 状态码存 STATUS，响应体写 $BODY
req() {
  local method=$1 path=$2 token=${3:-} body=${4:-}
  local args=(-s -o "$BODY" -w '%{http_code}' -X "$method" "$BASE$path")
  [ -n "$token" ] && args+=(-H "Authorization: Bearer $token")
  [ -n "$body" ] && args+=(-H "Content-Type: application/json" -d "$body")
  STATUS="$(curl "${args[@]}" || echo 000)"
}

# expect 描述 期望状态码
expect() {
  if [ "$STATUS" = "$2" ]; then ok "$1 ($2)"; else bad "$1：期望 ${2} 实得 ${STATUS}，响应 $(cat "$BODY")"; fi
}

# 无参时输出完整 JSON（供管道二次解析）；带路径时输出该字段值
field() {
  if [ $# -eq 0 ]; then
    python3 -c "import sys,json;print(json.dumps(json.load(sys.stdin)))" < "$BODY"
  else
    python3 -c "import sys,json;d=json.load(sys.stdin);print(d$1)" < "$BODY"
  fi
}

start_server() {
  DATABASE_URL="$DATABASE_URL" BIND_ADDR="$BIND_ADDR" cargo run -p bootstrap >"$LOG" 2>&1 &
  SRV_PID=$!
  for _ in $(seq 1 90); do
    if grep -q "服务启动完成" "$LOG"; then return 0; fi
    if ! kill -0 "$SRV_PID" 2>/dev/null; then
      echo "服务进程异常退出，日志：" >&2
      tail -20 "$LOG" >&2
      exit 1
    fi
    sleep 1
  done
  echo "90s 未就绪，日志：" >&2
  tail -20 "$LOG" >&2
  exit 1
}

stop_server() {
  if [ -n "${SRV_PID}" ]; then
    kill "${SRV_PID}" 2>/dev/null || true
    wait "${SRV_PID}" 2>/dev/null || true
    SRV_PID=""
  fi
}

STAMP="$(python3 -c 'import time;print(int(time.time()*1000))')"

# ---------- 1. 启动冒烟 ----------
say "1/7 启动冒烟"
start_server
ok "服务启动（bind=${BIND_ADDR}）"
# tracing 结构化字段带 ANSI 斜体转义，先剥掉再断言
LOG_TEXT="$(sed 's/\x1b\[[0-9;]*m//g' "$LOG")"
if echo "$LOG_TEXT" | grep -q "Skill 目录加载完成" && echo "$LOG_TEXT" | grep -q "count=2"; then
  ok "内置 Skill 加载（bilibili-video-notes + sports-training）"
else
  bad "Skill 加载异常：$(echo "$LOG_TEXT" | grep -o 'Skill.*' | head -1)"
fi
req GET /workspaces
expect "未带 token 访问受保护端点 → 401" 401
req GET /api/v1/nope
expect "未知 API 路径 → 404" 404

# ---------- 2. 认证与鉴权 ----------
say "2/7 认证与鉴权"
ACCOUNT="e2e_a_${STAMP}"
req POST /auth/register "" "{\"account\":\"${ACCOUNT}\",\"password\":\"password1\",\"nickname\":\"企业端到端A\"}"
expect "注册 → 200" 200
A_TOKEN="$(field "['token']")"
A_UID="$(field "['user']['id']")"
req POST /auth/register "" "{\"account\":\"${ACCOUNT}\",\"password\":\"password1\"}"
expect "重复注册 → 409" 409
req POST /auth/register "" '{"account":"e2e_weak","password":"short"}'
expect "弱密码 → 400" 400
req POST /auth/login "" "{\"account\":\"${ACCOUNT}\",\"password\":\"wrongpass\"}"
expect "错误密码登录 → 400" 400
req POST /auth/login "" "{\"account\":\"${ACCOUNT}\",\"password\":\"password1\"}"
expect "正确登录 → 200" 200
req POST /auth/logout "$A_TOKEN"
expect "退出登录 → 204（token 即吊销）" 204
# 注销后旧 token 失效，重登换新 token 供后续步骤使用
req POST /auth/login "" "{\"account\":\"${ACCOUNT}\",\"password\":\"password1\"}"
expect "注销后重新登录 → 200" 200
A_TOKEN="$(field "['token']")"

# ---------- 3. 体育学习闭环：空间 → 内容 → 刷题 → 错题 → 打卡 ----------
say "3/7 体育学习闭环（羽毛球）"
req POST /workspaces "$A_TOKEN" '{"name":"羽毛球备考","exam_goal":"正手高远球稳定过网","exam_date":"2026-12-01"}'
expect "创建备考空间 → 200" 200
WS_ID="$(field "['id']")"

# 播种 Agent 侧产物（模拟 MCP 写入的笔记与检验题）：一个笔记集 + 2 道单选题
ITEM_ID=$($PSQL -q "$DATABASE_URL" -t -A -c "
  insert into items (workspace_id, kind, name, content, created_by)
  values (${WS_ID}, 'note', '正手高远球要领', E'## 引拍\n手腕自然下垂…', 'agent')
  returning id;")
$PSQL -q "$DATABASE_URL" -t -A -c "
  insert into questions (workspace_id, source_item_id, type, stem, options, answer, explanation) values
  (${WS_ID}, ${ITEM_ID}, 'single', '引拍时拍头应？', '[\"紧贴身体\",\"自然下垂到背后\",\"举过头顶\"]', '1', '绕头引拍为后续鞭打蓄力'),
  (${WS_ID}, ${ITEM_ID}, 'single', '击球点在？', '[\"体侧\",\"身后\",\"身体前上方\"]', '2', '击球点在额头斜上方') returning id;" >/dev/null
Q_IDS=($($PSQL -q "$DATABASE_URL" -t -A -c "select id from questions where workspace_id=${WS_ID} order by id;"))
# 同步补一条 agent_write 事件：与 MCP write_item 路径写入的事件一致，复盘 Agent 靠它感知内容生产。
$PSQL -q "$DATABASE_URL" -t -A -c "insert into events (user_id, workspace_id, item_id, action, payload) values (${A_UID}, ${WS_ID}, ${ITEM_ID}, 'agent_write', '{\"item_id\":${ITEM_ID},\"note\":\"正手高远球要领\"}');" >/dev/null
ok "播种 1 个笔记集 + 2 道检验题（模拟 Agent 经 MCP 写入）"

req GET "/items/${ITEM_ID}" "$A_TOKEN"
expect "读取笔记（含要领内容）→ 200" 200
req GET "/quiz/questions?workspace_id=${WS_ID}&count=2" "$A_TOKEN"
expect "抽题 → 200" 200
if ! grep -q '"answer"' "$BODY"; then ok "抽题响应不含答案"; else bad "抽题响应泄露了答案"; fi
req POST /quiz/answer "$A_TOKEN" "{\"question_id\":${Q_IDS[0]},\"chosen\":1,\"scope\":\"第1集\"}"
expect "答对第 1 题 → 200" 200
[ "$(field "['is_correct']")" = "True" ] && ok "is_correct=true" || bad "第 1 题应判对"
req POST /quiz/answer "$A_TOKEN" "{\"question_id\":${Q_IDS[1]},\"chosen\":0}"
expect "答错第 2 题 → 200" 200
[ "$(field "['is_correct']")" = "False" ] && ok "is_correct=false" || bad "第 2 题应判错"
req GET /wrong "$A_TOKEN"
expect "错题本 → 200" 200
[ "$(field | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")" = "1" ] && ok "错题 1 条" || bad "错题应 1 条"
req GET /wrong/stats "$A_TOKEN"
expect "错题统计 → 200" 200
[ "$(field "['total']")" = "1" ] && ok "错题统计 total=1" || bad "错题统计 total 应为 1"

req POST /training/checkin "$A_TOKEN" '{"sport":"badminton","activity":"正手高远球","duration_minutes":60,"rating":4,"note":"手感不错"}'
expect "训练打卡（带备注）→ 200" 200
[ "$(field "['sport']")" = "badminton" ] && [ "$(field "['rating']")" = "4" ] && ok "打卡记录字段正确" || bad "打卡记录字段异常：$(cat "$BODY")"
req POST /training/checkin "$A_TOKEN" '{"sport":"core","activity":"平板支撑","duration_minutes":30,"rating":5}'
expect "训练打卡（无备注）→ 200" 200
req GET "/training/checkins?limit=10" "$A_TOKEN"
expect "打卡列表 → 200" 200
[ "$(field "[0]['sport']")" = "core" ] && ok "列表按时间倒序（最新 core 在前）" || bad "列表排序异常：$(cat "$BODY")"

# ---------- 4. 参数校验 ----------
say "4/7 参数校验"
for bad_body in \
  '{"sport":"badminton","activity":"x","duration_minutes":1,"rating":0}' \
  '{"sport":"badminton","activity":"x","duration_minutes":1,"rating":6}' \
  '{"sport":"  ","activity":"x","duration_minutes":1,"rating":3}' \
  '{"sport":"badminton","activity":" ","duration_minutes":1,"rating":3}' \
  '{"sport":"badminton","activity":"x","duration_minutes":0,"rating":3}'; do
  req POST /training/checkin "$A_TOKEN" "$bad_body"
  expect "非法打卡入参 → 400" 400
done
req GET "/training/checkins?limit=0" "$A_TOKEN"
expect "limit=0 → 400" 400
req GET "/training/checkins?limit=101" "$A_TOKEN"
expect "limit=101 → 400" 400
req POST /training/checkin "$A_TOKEN" '{"sport":"badminton","activity":"x","duration_minutes":1,"rating":3},bad'
expect "坏 JSON → 400" 400

# ---------- 5. 跨用户隔离 ----------
say "5/7 跨用户隔离"
req POST /auth/register "" "{\"account\":\"e2e_b_${STAMP}\",\"password\":\"password1\",\"nickname\":\"企业端到端B\"}"
expect "注册用户 B → 200" 200
B_TOKEN="$(field "['token']")"
req GET /workspaces "$B_TOKEN"
expect "B 的空间列表 → 200" 200
[ "$(field | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")" = "0" ] && ok "B 看不到 A 的空间" || bad "B 看到了 A 的空间"
req GET "/workspaces/${WS_ID}/tree" "$B_TOKEN"
expect "B 访问 A 的空间树 → 404" 404
req GET "/items/${ITEM_ID}" "$B_TOKEN"
expect "B 读 A 的笔记 → 404" 404
req GET "/training/checkins?limit=10" "$B_TOKEN"
expect "B 的打卡列表 → 200" 200
[ "$(field | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")" = "0" ] && ok "B 看不到 A 的打卡" || bad "B 看到了 A 的打卡"

# ---------- 6. 事件流落库（复盘 Agent 的数据源） ----------
say "6/7 事件流落库"
CNT=$($PSQL -q "$DATABASE_URL" -t -A -c "select count(*) from events where user_id=${A_UID} and action='checkin';")
[ "$CNT" = "2" ] && ok "events 表落 2 条 checkin 事件（action=checkin）" || bad "checkin 事件数应为 2，实得 ${CNT}"
PAYLOAD=$($PSQL -q "$DATABASE_URL" -t -A -c "select payload->>'sport' from events where user_id=${A_UID} and action='checkin' order by id limit 1;")
[ "$PAYLOAD" = "badminton" ] && ok "checkin 事件 payload 含 sport=badminton" || bad "payload 异常：${PAYLOAD}"
WRONG_CNT=$($PSQL -q "$DATABASE_URL" -t -A -c "select count(*) from events where user_id=${A_UID} and action='wrong';")
[ "$WRONG_CNT" -ge "1" ] && ok "错题也进事件流（action=wrong）" || bad "wrong 事件缺失"
AGENT_CNT=$($PSQL -q "$DATABASE_URL" -t -A -c "select count(*) from events where user_id=${A_UID} and action='agent_write';")
[ "$AGENT_CNT" -ge "1" ] && ok "AI 写内容也进事件流（action=agent_write）" || bad "agent_write 事件缺失"

# ---------- 7. 重启持久化 ----------
say "7/7 重启持久化"
stop_server
ok "服务已停止"
start_server
ok "服务重启完成"
req POST /auth/login "" "{\"account\":\"${ACCOUNT}\",\"password\":\"password1\"}"
expect "重启后登录 → 200" 200
A_TOKEN="$(field "['token']")"
req GET "/training/checkins?limit=10" "$A_TOKEN"
expect "重启后打卡列表 → 200" 200
[ "$(field | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")" = "2" ] && ok "重启后打卡记录仍在（PostgreSQL 持久化）" || bad "重启后打卡记录丢失"
req GET "/wrong" "$A_TOKEN"
expect "重启后错题本 → 200" 200
[ "$(field | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")" = "1" ] && ok "重启后错题仍在" || bad "重启后错题丢失"
req GET "/workspaces/${WS_ID}/tree" "$A_TOKEN"
expect "重启后空间树 → 200" 200

printf '\n\033[1;37m结果：%d 通过，%d 失败\033[0m\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
