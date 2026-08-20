---
name: "sports-video-notes"
description: "Turns sports movement-teaching videos into movement-essence notes where every knowledge point is paired with a screenshot, gif/webp animation, or mp4/webm clip, then uploads notes+media to the note platform via MCP. Invoke when the user gives a sports movement-teaching video link (badminton/core training) and asks for 动作要领/笔记/场景知识点. For knowledge course videos (soft exam/软件架构等), use bilibili-video-notes instead."
---

# 体育教学视频 → 动作要领笔记（每个知识点配图/动图/视频）

把体育教学视频（B 站等）转成结构化动作要领笔记，**每个「场景知识点」卡片都配上一张图 / 一段动图 / 一段视频片段**，最后整棵笔记树连同媒体附件一起传到学伴笔记平台。

## 触发场景
- 用户给出体育教学视频链接（羽毛球 高远球/步法、核心训练 平板支撑/卷腹/臀桥 等）并要求"做笔记 / 整理动作要领 / 记录场景知识点"
- 用户说"用体育视频笔记 skill"或类似表述

## 核心原则
1. **动作要领必须来自视频本身**：技术阶段拆解、错误纠正、教练强调均以视频讲解为准，禁止用网络知识替换视频内容；画面信息不足处可结合 `references/` 参考库补充，但须标注"（补充）"。
2. **语言一致**：输出语言与用户最新输入一致。
3. **一知识点一媒体（硬性要求）**：每个场景知识点（技术阶段 / 常见错误 / 教练强调）至少配 1 个媒体附件——静态关键帧截图、gif/webp 动图，或 mp4/webm 短视频片段，三者至少其一。
4. **媒体走 `upload_attachment`**：截图/动图/视频统一 base64 上传，拿到 `/api/v1/attachments/{id}` 后用 `![alt](url)` 嵌入笔记正文（图片/动图/视频同一写法）。
5. **时间戳可回溯**：每个知识点标注视频时间戳。

## 依赖与脚本
- 中间产物（视频/音频/帧/动图/片段）→ 临时目录 `/Users/kangliu/.trae-cn/work/<session>`
- 复用 `bilibili-video-notes` skill 的脚本（下载/转写/截帧）；动图与视频片段用 ffmpeg 命令截取（见阶段 3）
- 依赖检查：
```bash
command -v yt-dlp || pip install yt-dlp --break-system-packages
command -v ffmpeg || brew install ffmpeg
python3 -c "import vosk" 2>/dev/null || pip install vosk --break-system-packages
```

## 流程

### 阶段 0 · 环境检查与下载
```bash
bash <skills>/bilibili-video-notes/scripts/download.sh "<bilibili_url>" "<workdir>"
```
输出 `<workdir>/video.mp4`。B 站 `p` 参数从 1 开始，下载后用实际视频标题核对集数。

### 阶段 1 · 提取音频并转写
```bash
bash <skills>/bilibili-video-notes/scripts/prepare_audio.sh "<workdir>/video.mp4" "<workdir>/audio.wav"
python3 <skills>/bilibili-video-notes/scripts/transcribe.py "<workdir>/audio.wav" "<workdir>/transcript.txt"
```
转写关注**动作指令性语句**：手/脚/重心/呼吸的位置与时机、发力顺序、常见错误描述。输出带时间戳的 `transcript.txt`。

### 阶段 2 · 场景检测截帧
```bash
bash <skills>/bilibili-video-notes/scripts/frames.sh "<workdir>/video.mp4" "<workdir>/frames" 0.08
```
逐帧读取画面：动作示范截图、正误对比图、教练标注（箭头/圈注/文字）。帧太少调低阈值到 0.05，太多调到 0.1~0.15。

### 阶段 3 · 为知识点抽取媒体（关键）
对每个待记的场景知识点，按视频时间戳抽取对应媒体，三选一（能动人优先）：
```bash
# 静态关键帧（技术阶段定格）
ffmpeg -ss <MM:SS> -i video.mp4 -frames:v 1 -q:v 2 "<workdir>/media/<动作>_<阶段>.jpg"
# 动图 gif（连续发力/步法动作，3 秒内）
ffmpeg -ss <MM:SS> -t 3 -i video.mp4 -vf "fps=10,scale=480:-2:flags=lanczos" "<workdir>/media/<动作>_<阶段>.gif"
# 动图 webp（体积更小，优先）
ffmpeg -ss <MM:SS> -t 3 -i video.mp4 -vf "fps=10,scale=480:-2:flags=lanczos" -loop 0 "<workdir>/media/<动作>_<阶段>.webp"
# 视频片段 mp4（动作示范，3~8 秒，转 H.264 保证网页/安卓可播）
ffmpeg -ss <MM:SS> -t 6 -i video.mp4 -c:v libx264 -pix_fmt yuv420p -movflags +faststart "<workdir>/media/<动作>_<阶段>.mp4"
```
命名规范 `<动作>_<阶段>_<序号>`，便于检索。单附件 ≤10MB，超过则缩短时长或降分辨率。

### 阶段 4 · 构建笔记结构
一个动作 = 一篇笔记，结构：
1. **动作概述**：一句话说清练什么、适用人群
2. **技术阶段拆解**：准备 → 引拍/预备 → 发力 → 完成（还原），每阶段要点 + 常见错误，**每阶段配一张图/动图**
3. **常见错误与纠正**：错误表现 → 危害 → 纠正方法，**每条配正误对比图**
4. **教练强调**：教练反复强调的细节 + 时间戳，**配示范动图/视频**
5. **训练建议**：组数/次数/时长、进阶路线

### 阶段 5 · 上传笔记平台（MCP）
按顺序调用，把内容与媒体一起落地：
1. `create_workspace(name, exam_goal, exam_date?)` 建空间（如 `羽毛球` / `exam_goal=正手高远球`），拿 `workspace_id`
2. `create_item(workspace_id, parent_id=null, kind="dir", name=<动作>训练, content=null)` 建目录
3. `create_item(workspace_id, parent_id=<目录id>, kind="note", name=<动作>, content=null)` 建笔记，拿 `item_id`
4. 逐个媒体文件 `upload_attachment(item_id, filename, content_base64)`，记下返回的 `url`
5. 组装 Markdown 正文，把 `![alt](url)` 按段落插入，`write_item(item_id, content)` 落正文

示例正文片段：
```markdown
## 发力
蹬地转髋 → 转肩 → 大臂带动小臂 → 手腕内旋闪动，击球点在右肩前上方最高点。
![发力顺序示范](/api/v1/attachments/12)
```

## 参考库
- `references/badminton.md`：羽毛球内容种子（高远球、步法）
- `references/core-training.md`：核心训练内容种子（平板支撑、卷腹、臀桥）

## 常见问题
- **视频无动作示范画面**：以语音讲解为主干，画面缺失步骤标注"（画面缺失，建议对照参考库）"，并优先用 gif/视频补足相邻动作。
- **下载失败**：720p 通常免登录；需要登录时请用户提供 cookies 后重跑 `download.sh <url> <workdir> <cookies>`。
- **动图/视频体积超 10MB**：缩短片段时长（3~5 秒）、降 fps 到 8、降分辨率到 360p，或改用静态关键帧。
