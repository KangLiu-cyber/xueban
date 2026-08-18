---
name: "sports-training"
description: "Sports education domain pack for badminton and core training: turns teaching videos into movement-essence notes (with images/animations) plus check questions, and reviews training check-ins to diagnose weak points. Invoke when the user provides a sports teaching video link (badminton 高远球/步法, core training 平板支撑/卷腹/臀桥) and asks for 动作要领/笔记/检验题, or asks for training review/复盘 after check-ins."
---

# 体育训练：动作要领笔记 + 检验题 + 复盘

体育领域包最小闭环：**教学视频 → 动作要领笔记（图/动图）→ 要领检验题 → 刷题错题归集 → 训练打卡 → AI 复盘改进**。

## 触发场景
- 用户给出体育教学视频链接（B 站 / 其他）并要求"做笔记 / 整理动作要领 / 出检验题"
- 用户说"用体育训练 skill"或类似表述
- 用户要求"复盘 / 改进训练"（此时先读打卡与错题事件）

## 核心原则
1. **动作要领必须来自视频本身**：技术阶段拆解、错误纠正、教练强调均以视频讲解为准，禁止用网络知识替换视频内容；视频画面信息不足的部分可结合参考库（`references/`）与运动常识补充，但需注明"（补充）"。
2. **语言一致**：输出语言与用户最新输入一致。
3. **图/动图优先**：动作要点尽量配图（截图）或动图（gif/webp，阶段一不支持 mp4 长视频），经 `upload_attachment` 上传后嵌入笔记。
4. **每集出检验题**：动作要领笔记完成后用 `save_questions` 出 5~10 道检验题（含解析），落入题库自动进刷题/错题闭环。
5. **复盘基于事件**：复盘时用 `get_events` 读 `checkin`（打卡）与 `wrong`（错题）事件，数据驱动诊断，不空谈。

## 工作目录
- 中间产物（视频/音频/帧/转写）→ 临时目录 `/Users/kangliu/.trae-cn/work/<session>`
- 最终笔记 → 用户工作区（final workspace folder）

## 流程

### 流程 A · 教学视频 → 动作要领笔记 + 检验题

#### 阶段 1 · 环境检查与下载
依赖缺失则安装（与 bilibili-video-notes skill 相同脚本能力）：
```bash
command -v yt-dlp || pip install yt-dlp --break-system-packages
command -v ffmpeg || brew install ffmpeg
python3 -c "import vosk" 2>/dev/null || pip install vosk --break-system-packages
```
下载视频到工作目录（B 站 `p` 参数偏移坑同 bilibili-video-notes skill，用实际标题核对集数）。

#### 阶段 2 · 提取动作信息
- 提取音频并转写（带时间戳），**关注动作指令性语句**：手/脚/重心/呼吸的位置与时机、发力顺序、常见错误描述
- 场景检测截帧，逐帧读取画面：动作示范截图、正误对比图、教练标注（箭头/圈注/文字）

#### 阶段 3 · 写动作要领笔记
用 `write_item` 写笔记，结构：
1. **动作概述**：一句话说清这个动作练什么、适用人群
2. **技术阶段拆解**：准备 → 引拍/预备 → 发力 → 完成（还原），每阶段含要点与常见错误（表格式优先）
3. **常见错误与纠正**：错误表现 → 危害 → 纠正方法（可配正误对比图）
4. **教练强调**：视频中教练反复强调的细节，标注时间戳可回溯
5. **训练建议**：组数/次数/时长、进阶路线
- 动作示范截图、正误对比图、动图（gif/webp）用 `upload_attachment` 上传后以图片形式嵌入对应段落；图片命名用 `<动作>_<阶段>_<序号>` 便于检索
- 视频无法截图成动图的场景，可让用户自行录制动图后上传

#### 阶段 4 · 出检验题
用 `save_questions` 出 5~10 道，覆盖：
- 技术要点（占 60%）："引拍时手腕应……"、"发力顺序是……"
- 常见错误识别（占 30%）："下列哪个是常见错误？"
- 训练安排（占 10%）：组数/次数
每题带解析（为什么对、为什么错）。题型单选/多选/判断均可，与学伴题库格式一致。

### 流程 B · 复盘（训练打卡后）
1. **读事件**：`get_events`（limit 50+），过滤 `action=checkin`（打卡：`payload` 含 sport/activity/duration_minutes/rating/note）与 `action=wrong`（错题：可结合 `list_items` 查错题内容）
2. **诊断**：按运动 / 时长 / 自评 / 错题主题定位薄弱环节，例如：
   - 羽毛球自评连续低分 → 回看对应要领笔记，找大概率错误的环节
   - 错题集中在某动作阶段 → 该阶段要领没掌握
   - 打卡频率低 / 时长不足 → 训练量问题，给可行性建议
3. **写回**：用 `write_item` 写一篇"纠正讲解"笔记（错误 → 为什么错 → 怎么改，附对应要领段落），用 `add_annotation` 在原始要领笔记的错误相关段落加批注
4. **出新题**：针对薄弱环节用 `save_questions` 出 3~5 道针对性检验题
5. **个性化建议**：根据薄弱点给出具体训练建议（如"本周每次练完加 10 分钟高远球多球练习，下次打卡记录发力手感"）

## 参考库
- `references/badminton.md`：羽毛球内容种子（高远球、步法）
- `references/core-training.md`：核心训练内容种子（平板支撑、卷腹、臀桥）

## 常见问题
- **视频无动作示范画面**：以语音讲解为主干整理要领，画面缺失的步骤标注"（画面缺失，建议对照参考库）"。
- **用户打卡但复盘请求笼统**：先读事件给数据画像（近 N 次运动/时长/自评/错题分布），再问用户想改进哪方面，避免空泛建议。
- **检验题误判**：体育动作判断题要注意表述歧义（如"手腕"vs"小臂"），题干写具体，解析写明判断依据。
