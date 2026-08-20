---
name: "sports-review"
description: "Reviews a learner's submitted training video for a video-answer question (视频作答题) and writes back a movement-correction review note. Reads the lesson note, the learner's training idea text, and the submitted video (frame-sampled), then creates a 复盘 note under the same lesson directory. Invoke when the user asks to 复盘/批改/点评 a submitted sports training video, or says to use the sports-review skill."
---

# 体育训练视频 → 动作复盘笔记

读取用户在「视频作答题」中提交的训练视频与训练想法，结合该题所属的讲义（动作要领笔记），生成一篇**动作复盘笔记**写回系统，挂在讲义同一目录下。

## 触发场景
- 用户交完视频作业后说「帮我复盘 / 批改 / 点评一下我的训练视频」
- 用户说「用体育复盘 skill」或类似表述

## 核心原则
1. **复盘必须基于讲义 + 视频画面 + 用户训练想法**：纠正要点以讲义的技术阶段与常见错误为对照，从视频帧里读出用户实际动作问题，禁止凭空编造用户没做的动作。视频画面信息不足处标注「（视频角度/清晰度受限，建议重拍）」，不要臆测。
2. **语言一致**：输出语言与用户最新输入一致。
3. **抽帧再看**：先对视频抽关键帧（见阶段 2），逐帧对照讲义给出纠正；帧太少就加密抽样。
4. **复盘落成一篇新笔记**：用 `create_item` 在当前讲义所在目录下新建一篇「复盘」笔记，再用 `write_item` 写正文，不要改动原讲义。

## 依赖与脚本
- 中间产物（视频/帧）→ 临时目录 `/Users/kangliu/.trae-cn/work/<session>`
- 依赖检查：
```bash
command -v ffmpeg || brew install ffmpeg
```

## 流程

### 阶段 1 · 找到待复盘的视频作业
经 MCP `get_events` 读取最近事件，过滤 `action == "video_submit"`：
```text
get_events(limit=50)
```
- 事件 `payload` 含：`question_id`、`attachment_ids`（视频/图片附件 id 数组）、`note`（训练想法，可空）。
- 事件 `item_id` 即题源笔记 id（讲义）。若用户指定了某篇笔记，优先复盘与其 `item_id` 匹配的最新一条作业。

### 阶段 2 · 读讲义 + 读视频 + 抽帧
1. `read_item(item_id)` 读讲义正文，提炼技术阶段、常见错误、教练强调，作为对照基准。
2. 对每个附件 `read_attachment(attachment_id)` 取回二进制（base64 + mime）：
   - `image/*`：直接解码为图片查看。
   - `video/*`：base64 解码写盘 `<workdir>/submit.mp4`，再抽关键帧：
```bash
mkdir -p <workdir>/frames
# 每 0.5 秒抽一帧，覆盖关键动作（可先看时长再调）
ffmpeg -i <workdir>/submit.mp4 -vf "fps=2,scale=640:-2:flags=lanczos" -q:v 3 <workdir>/frames/f%03d.jpg
```
   - 逐帧读取画面，结合讲义对照：准备/引拍/发力/完成各阶段动作是否到位、有无常见错误（如重心前冲、手腕没内旋、转体不充分）。
3. 记录 `note` 里的训练想法，作为用户自述的问题线索优先回应。

### 阶段 3 · 生成复盘正文
复盘笔记结构：
1. **整体评价**：一句话总评（哪里做得好、最需要改的一处）。
2. **逐阶段纠正**：按讲义技术阶段逐条对照——「你的动作」vs「正确要领」vs「怎么改」，每条结合视频帧证据。
3. **针对训练想法的回应**：若用户写了训练想法（如「发力用不上腰」），逐条给出原因与纠正。
4. **训练建议**：2~3 条可执行的下一步练习（组数/次数/关注点）。

示例正文片段：
```markdown
## 发力阶段
- 你的动作：击球前重心已前冲，转体未完成就出手（见 00:12 帧）。
- 正确要领：蹬地转髋 → 转肩 → 大臂带动小臂 → 手腕内旋闪动。
- 怎么改：先徒手练「蹬地转髋」分解，身体转正后再挥拍，暂不追求力量。
```

### 阶段 4 · 写回复盘笔记
```text
create_item(workspace_id, parent_id=<讲义所在目录 id>, kind="note", name="复盘 · <动作名>", content=null)
write_item(item_id=<新笔记 id>, content=<复盘正文>)
```
- `parent_id` 取讲义的父目录 id（若讲义是根节点则 parent_id=null）；`workspace_id` 从事件或讲义信息获取。
- 复盘正文可插入抽帧截图：先用 `upload_attachment` 上传关键帧，再 `![说明](/api/v1/attachments/{id})` 嵌入对应段落（可选，画面对照更直观）。

## 常见问题
- **视频抽不到帧/解码失败**：改用训练想法 + 讲义做文字复盘，并提示用户重传更清晰的视频。
- **事件里没有 video_submit**：提示用户先去刷题页完成视频题作答再复盘。
- **讲义缺失**：提示先用 `sports-video-notes` 补讲义，复盘才有对照基准。
