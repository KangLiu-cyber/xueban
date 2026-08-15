---
name: "bilibili-video-notes"
description: "Extracts study notes from Bilibili course videos by transcribing the teacher's audio (Vosk) and reading PPT slides from scene-detected keyframes, then fills a clean HTML note template. Invoke when the user provides a bilibili.com video link and asks for notes/笔记/重点, or says to use the video-note skill."
---

# Bilibili 视频笔记提取

把 B 站课程视频转成结构化学习笔记。**内容严格来自视频本身**（老师语音 + PPT 画面），禁止掺入网络搜索或模型自身知识。

## 触发场景
- 用户给出 `bilibili.com/video` 链接并要求"做笔记 / 整理重点 / 提取内容"
- 用户说"用视频笔记 skill"或类似表述

## 核心原则
1. **只用视频内容**：所有知识点必须来自语音转写或 PPT 画面，不得用外部知识或网络搜索补充。
2. **语言一致**：输出语言与用户最新输入一致。
3. **时间戳对应**：每个知识点标注视频时间戳，可回溯。
4. **并发限制**：如需子代理探索，同时不超过 3 个 Explore 子代理。

## 工作目录
- 中间产物（视频/音频/帧/转写）→ 临时目录 `/Users/kangliu/.trae-cn/work/<session>`
- 最终笔记 → 用户工作区（final workspace folder）

## 流程

### 阶段0 · 环境检查
检查依赖，缺失则安装：
```bash
command -v yt-dlp || pip install yt-dlp --break-system-packages
command -v ffmpeg || brew install ffmpeg
python3 -c "import vosk" 2>/dev/null || pip install vosk --break-system-packages
```
脚本位于本 skill 的 `scripts/` 目录，直接调用即可。

### 阶段1 · 解析链接并下载
**关键坑：p 参数偏移**。B 站 `p` 从 1 开始，但有些课程集数标题从 0 开始，下载后务必用实际视频标题核对集数。
```bash
bash scripts/download.sh "<bilibili_url>" "<workdir>"
```
输出 `<workdir>/video.mp4`。

### 阶段2 · 提取音频并转写
```bash
bash scripts/prepare_audio.sh "<workdir>/video.mp4" "<workdir>/audio.wav"
python3 scripts/transcribe.py "<workdir>/audio.wav" "<workdir>/transcript.txt"
```
输出带时间戳的逐句文字 `transcript.txt`（格式 `[MM:SS] 内容`）。

### 阶段3 · 场景检测截帧
```bash
bash scripts/frames.sh "<workdir>/video.mp4" "<workdir>/frames" 0.08
```
自动抓 PPT 翻页关键帧，并生成 `frames/timestamps.txt`（与帧序号一一对应）。
- 帧太少 → 阈值调低到 0.05
- 帧太多 → 阈值调高到 0.1~0.15

### 阶段4 · 读取画面
逐帧读取 `frames/`，提取四类内容：
- PPT 正文文字
- 架构图 / 示意图及标签
- 老师红色手写批注（星号 / 下划线 / 圈注 / 补充字）
- 底部字幕条

### 阶段5 · 整合并套用模板
把语音转写 + 画面内容按时间轴对齐，填入 `references/note-template.html` 模板，结构：
Header → 本集框架 → 知识点卡片 → 速记表 → 考点提示 → 核心速记。
每张知识点卡片包含：星级、标题、时间戳、一句话核心、要点、图解（可选）、老师强调、记忆技巧。

### 阶段6 · 输出
保存到用户工作区，命名 `<课程>_<集数>_<标题>_笔记.html`，并用 computer:// 链接分享给用户。

## 星级体系
优先按老师视频中的标星：★★★必须掌握 / ★★尽力掌握 / ★加深印象 / 无星了解。
若老师未标星，按出题频率与强调程度自行判断，并在卡片中注明"（推断）"。

## 常见问题
- **需要登录 / 大会员**：720p 通常免登录；若下载失败，请用户提供 cookies 文件后重跑 `download.sh <url> <workdir> <cookies>`。
- **无语音 / 纯音乐**：转写为空时，仅依赖 PPT 画面 + 字幕条整理。
- **转写不准**：可换用 faster-whisper（不依赖 PyTorch）提升精度；Vosk 小模型约 40MB，首次运行会自动下载。
- **视频无字幕且语音快**：以 PPT 画面为主干，语音转写作为补充说明。
