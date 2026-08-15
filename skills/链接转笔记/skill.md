---
name: 链接转笔记
description: 把网页或视频链接的内容整理成结构化笔记
---
步骤1：解析用户提供的链接，抓取页面/视频的标题与正文要点。
步骤2：按「核心概念 → 分节要点 → 关键结论」组织成 Markdown 框架笔记。
步骤3：用 create_workspace 确认空间存在后，用 create_item 创建目录（kind=dir），
       再用 create_item 创建笔记（kind=note，parent_id 指向目录），最后用 write_item 写入正文。
步骤4：每篇笔记控制在 500 字以内，保留原文关键术语与数据。
