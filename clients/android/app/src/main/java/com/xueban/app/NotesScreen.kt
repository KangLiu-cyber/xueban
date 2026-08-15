package com.xueban.app

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// ==================== 课程 / 集推导（镜像 web state.rs derive_courses / collect_episodes） ====================

/** 集：带笔记子节点的目录（openId = 首个笔记），或直接是笔记。 */
data class EpUi(val nodeId: Long, val openId: Long, val title: String, val no: Int, val noteIds: List<Long>)

data class CourseUi(val dirId: Long, val name: String, val episodes: List<EpUi>)

/** 从树推导课程 / 集：workspace → 课程 dir →（可折叠 folder dir）→ 集节点。编号在课程内重排。 */
fun deriveCourses(tree: List<ItemNode>): List<CourseUi> {
    val courses = mutableListOf<CourseUi>()
    for (node in tree) {
        if (node.item.kind != ItemKind.Dir) continue
        val eps = mutableListOf<EpUi>()
        collectEps(node.children, eps)
        if (eps.isEmpty()) continue
        courses.add(CourseUi(node.item.id, node.item.name, eps))
    }
    return courses
}

private fun collectEps(children: List<ItemNode>, out: MutableList<EpUi>) {
    for (child in children) {
        when (child.item.kind) {
            ItemKind.Dir -> {
                val notes = child.children.filter { it.item.kind == ItemKind.Note }
                if (notes.isNotEmpty()) {
                    out.add(
                        EpUi(
                            nodeId = child.item.id,
                            openId = notes.first().item.id,
                            title = child.item.name,
                            no = out.size + 1,
                            noteIds = notes.map { it.item.id },
                        )
                    )
                } else {
                    collectEps(child.children, out)
                }
            }
            ItemKind.Note -> out.add(
                EpUi(
                    nodeId = child.item.id,
                    openId = child.item.id,
                    title = child.item.name,
                    no = out.size + 1,
                    noteIds = listOf(child.item.id),
                )
            )
        }
    }
}

/** 集范围名：单课程 '第{n}集 {title}'。 */
fun epScopeName(ep: EpUi): String = "第${ep.no}集 ${ep.title}"

/** 所有集（扁平），供刷题范围选择。 */
fun allEps(courses: List<CourseUi>): List<Pair<CourseUi, EpUi>> {
    val out = mutableListOf<Pair<CourseUi, EpUi>>()
    for (c in courses) for (e in c.episodes) out.add(c to e)
    return out
}

// ==================== Markdown → Compose 块（镜像 web markdown.rs） ====================

private sealed class InlinePart {
    class Plain(val text: String) : InlinePart()
    class Bold(val text: String) : InlinePart()
    class Code(val text: String) : InlinePart()
    class Italic(val text: String) : InlinePart()
}

/** 行内格式：`code` → **bold** → *italic*，顺序扫描不做嵌套。 */
private fun parseInline(src: String): List<InlinePart> {
    val parts = mutableListOf<InlinePart>()
    var s = src
    // code
    while (true) {
        val pos = s.indexOf('`')
        if (pos < 0) {
            parts.add(InlinePart.Plain(s)); s = ""; break
        }
        if (pos > 0) parts.add(InlinePart.Plain(s.substring(0, pos)))
        val end = s.indexOf('`', pos + 1)
        if (end < 0) {
            parts.add(InlinePart.Plain("`" + s.substring(pos + 1)))
            s = ""; break
        }
        parts.add(InlinePart.Code(s.substring(pos + 1, end)))
        s = s.substring(end + 1)
    }
    // bold
    val withBold = mutableListOf<InlinePart>()
    for (p in parts) {
        if (p !is InlinePart.Plain) { withBold.add(p); continue }
        var rest = p.text
        while (true) {
            val pos = rest.indexOf("**")
            if (pos < 0) { withBold.add(InlinePart.Plain(rest)); break }
            if (pos > 0) withBold.add(InlinePart.Plain(rest.substring(0, pos)))
            val end = rest.indexOf("**", pos + 2)
            if (end < 0) { withBold.add(InlinePart.Plain("**" + rest.substring(pos + 2))); break }
            withBold.add(InlinePart.Bold(rest.substring(pos + 2, end)))
            rest = rest.substring(end + 2)
        }
    }
    // italic
    val out = mutableListOf<InlinePart>()
    for (p in withBold) {
        if (p !is InlinePart.Plain) { out.add(p); continue }
        var rest = p.text
        while (true) {
            val pos = rest.indexOf('*')
            if (pos < 0) { out.add(InlinePart.Plain(rest)); break }
            if (pos > 0) out.add(InlinePart.Plain(rest.substring(0, pos)))
            val end = rest.indexOf('*', pos + 1)
            if (end < 0) { out.add(InlinePart.Plain("*" + rest.substring(pos + 1))); break }
            out.add(InlinePart.Italic(rest.substring(pos + 1, end)))
            rest = rest.substring(end + 1)
        }
    }
    return out
}

/** 纯文本（去行内标记），用于批注引文。 */
private fun inlinePlain(src: String): String =
    parseInline(src).joinToString("") { p ->
        when (p) {
            is InlinePart.Plain -> p.text
            is InlinePart.Bold -> p.text
            is InlinePart.Code -> p.text
            is InlinePart.Italic -> p.text
        }
    }

private sealed class MdBlock {
    class Heading(val level: Int, val content: String) : MdBlock()
    class Para(val content: String) : MdBlock()
    class List(val ordered: Boolean, val items: kotlin.collections.List<String>) : MdBlock()
    class Table(val rows: kotlin.collections.List<kotlin.collections.List<String>>) : MdBlock()
    class Tip(val lines: kotlin.collections.List<String>) : MdBlock()
}

private fun headingLevel(t: String): Int? {
    val trimmed = t.trimStart()
    if (!trimmed.startsWith('#')) return null
    var n = 0
    for (c in trimmed) {
        if (c == '#') n++ else break
    }
    if (n !in 1..6) return null
    return n.coerceIn(2, 4)
}

private fun isTableSep(line: String): Boolean {
    val t = line.trim()
    if (t.isEmpty()) return false
    val body = t.trim('|').trim()
    return body.isNotEmpty() &&
        body.split('|').all { c -> val x = c.trim(); x.startsWith(':') || x.endsWith(':') || x == "-" } &&
        body.contains('-')
}

private fun looksLikeTableRow(line: String): Boolean {
    val t = line.trim()
    return t.startsWith('|') && t.endsWith('|') && t.count { it == '|' } >= 2
}

private fun renderTable(lines: List<String>, i: Int): Pair<List<List<String>>, Int> {
    val rows = mutableListOf<List<String>>()
    var consumed = 0
    var started = false
    var j = i
    while (j < lines.size) {
        val t = lines[j].trim()
        if (!started) {
            if (!(looksLikeTableRow(t) || t.contains('|'))) break
            if (j + 1 < lines.size && isTableSep(lines[j + 1])) {
                started = true
                rows.add(splitCells(t))
                consumed++
                j++
                continue
            }
            break
        }
        if (!looksLikeTableRow(t)) break
        rows.add(splitCells(t))
        consumed++
        j++
    }
    return rows to consumed
}

private fun splitCells(line: String): List<String> =
    line.trim().trim('|').split('|').map { inlinePlain(it.trim()) }

private fun parseMarkdown(src: String): List<MdBlock> {
    val lines = src.lines()
    val blocks = mutableListOf<MdBlock>()
    var i = 0
    while (i < lines.size) {
        val line = lines[i]
        val t = line.trim()
        if (t.isEmpty()) { i++; continue }
        val h = headingLevel(t)
        if (h != null) {
            blocks.add(MdBlock.Heading(h, inlinePlain(t.substringAfter('#'))))
            i++
            continue
        }
        if (isTableSep(line)) { i++; continue }
        if (looksLikeTableRow(t) && i + 1 < lines.size && isTableSep(lines[i + 1])) {
            val (rows, n) = renderTable(lines, i)
            if (rows.isNotEmpty()) blocks.add(MdBlock.Table(rows))
            i += n
            continue
        }
        if (t.startsWith("- ") || t.startsWith("* ")) {
            val items = mutableListOf<String>()
            while (i < lines.size) {
                val l = lines[i].trim()
                val item = l.removePrefix("- ").takeIf { l.startsWith("- ") }
                    ?: l.removePrefix("* ").takeIf { l.startsWith("* ") } ?: break
                items.add(inlinePlain(item))
                i++
            }
            blocks.add(MdBlock.List(false, items))
            continue
        }
        if (t.startsWith(">")) {
            val parts = mutableListOf<String>()
            while (i < lines.size) {
                val l = lines[i].trim()
                if (l.startsWith(">")) { parts.add(inlinePlain(l.substring(1).trim())); i++ }
                else if (l.isEmpty()) { i++ }
                else break
            }
            blocks.add(MdBlock.Tip(parts))
            continue
        }
        if (t.firstOrNull()?.isDigit() == true && t.contains(". ")) {
            val items = mutableListOf<String>()
            while (i < lines.size) {
                val l = lines[i].trim()
                val item = l.split(". ", limit = 2).getOrNull(1)?.trim()
                if (l.firstOrNull()?.isDigit() == true && item != null) { items.add(inlinePlain(item)); i++ }
                else break
            }
            if (items.isNotEmpty()) { blocks.add(MdBlock.List(true, items)); continue }
        }
        // 段落：收集直到空行或块级起始
        val para = mutableListOf<String>()
        while (i < lines.size) {
            val l = lines[i].trim()
            if (l.isEmpty() || headingLevel(l) != null || looksLikeTableRow(l) ||
                l.startsWith(">") || l.startsWith("- ") || l.startsWith("* ")
            ) break
            para.add(inlinePlain(l))
            i++
        }
        if (para.isNotEmpty()) blocks.add(MdBlock.Para(para.joinToString(" ")))
    }
    return blocks
}

// ---- 批注锚点匹配（光标跟踪，跳过未找到的锚点，避免重复注入） ----

private class AnnoSpan(val start: Int, val end: Int, val anno: Annotation)

private fun annoChunks(src: String, annos: List<Annotation>): List<Pair<String, Annotation?>> {
    val spans = mutableListOf<AnnoSpan>()
    var rest = src
    var off = 0
    for (a in annos) {
        val i = rest.indexOf(a.anchor)
        if (i < 0) continue
        spans.add(AnnoSpan(off + i, off + i + a.anchor.length, a))
        rest = rest.substring(i + a.anchor.length)
        off += i + a.anchor.length
    }
    if (spans.isEmpty()) return listOf(src to null)
    val chunks = mutableListOf<Pair<String, Annotation?>>()
    var pos = 0
    for (s in spans.sortedBy { it.start }) {
        if (s.start > pos) chunks.add(src.substring(pos, s.start) to null)
        chunks.add(src.substring(s.start, s.end) to s.anno)
        pos = s.end
    }
    if (pos < src.length) chunks.add(src.substring(pos) to null)
    return chunks
}

/** 行内渲染为 AnnotatedString：粗体 / 代码 / 斜体 + 批注金色高亮可点击。 */
private fun renderInlineAnnotated(src: String, annos: List<Annotation>, onAnno: (Annotation) -> Unit): AnnotatedString {
    val b = AnnotatedString.Builder()
    var tag = 0
    for ((text, anno) in annoChunks(src, annos)) {
        val start = b.length
        for (p in parseInline(text)) {
            val s = b.length
            when (p) {
                is InlinePart.Plain -> b.append(p.text)
                is InlinePart.Bold -> { b.addStyle(SpanStyle(fontWeight = FontWeight.Bold), s, s + p.text.length); b.append(p.text) }
                is InlinePart.Code -> { b.addStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = Xb.surface2), s, s + p.text.length); b.append(p.text) }
                is InlinePart.Italic -> { b.addStyle(SpanStyle(fontStyle = FontStyle.Italic), s, s + p.text.length); b.append(p.text) }
            }
        }
        if (anno != null) {
            val mine = anno.author == AnnotationAuthor.User
            b.addStyle(
                SpanStyle(
                    background = if (mine) Xb.accentLight else Xb.goldLight,
                    fontWeight = FontWeight.SemiBold,
                ),
                start,
                b.length,
            )
            b.addLink(
                LinkAnnotation.Clickable("anno-${tag++}", TextLinkStyles(), { onAnno(anno) }),
                start,
                b.length,
            )
        }
    }
    return b.toAnnotatedString()
}

/** kcard 拆分为（标题，正文）：段落整体以 ** 开头且无批注时按原型知识卡片渲染。 */
private fun kcardParts(content: String): Pair<String, AnnotatedString>? {
    val parts = parseInline(content)
    val title = StringBuilder()
    var i = 0
    while (i < parts.size && parts[i] is InlinePart.Bold) {
        title.append((parts[i] as InlinePart.Bold).text)
        i++
    }
    if (i == 0 || i >= parts.size) return null
    val b = AnnotatedString.Builder()
    for (p in parts.drop(i)) {
        when (p) {
            is InlinePart.Plain -> b.append(p.text)
            is InlinePart.Bold -> { b.addStyle(SpanStyle(fontWeight = FontWeight.Bold), b.length, b.length + p.text.length); b.append(p.text) }
            is InlinePart.Code -> { b.addStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = Xb.surface2), b.length, b.length + p.text.length); b.append(p.text) }
            is InlinePart.Italic -> { b.addStyle(SpanStyle(fontStyle = FontStyle.Italic), b.length, b.length + p.text.length); b.append(p.text) }
        }
    }
    return title.toString() to b.toAnnotatedString()
}

// ==================== 笔记页 ====================

@Composable
fun NotesScreen(state: AppState, onTabToMe: () -> Unit) {
    val courses = remember(state.tree) { deriveCourses(state.tree) }
    var openCourseId by remember { mutableStateOf<Long?>(null) }
    LaunchedEffect(courses.size) {
        if (openCourseId == null && courses.isNotEmpty()) openCourseId = courses[0].dirId
    }
    var openEpNodeId by remember { mutableStateOf<Long?>(null) }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 600.dp
        val openEp: (EpUi) -> Unit = { ep ->
            openEpNodeId = ep.nodeId
            state.openNote(ep.openId)
        }
        if (wide) {
            Row(Modifier.fillMaxSize()) {
                DirPane(
                    state, courses, openCourseId, { openCourseId = it },
                    openEpNodeId, openEp, onTabToMe,
                    Modifier.width(316.dp).fillMaxSize(),
                )
                NotePane(state, Modifier.weight(1f), showBack = false, bottomPad = 24.dp)
            }
        } else if (state.noteOpen) {
            NotePane(state, Modifier.fillMaxSize(), showBack = true, bottomPad = 110.dp)
        } else {
            DirPane(
                state, courses, openCourseId, { openCourseId = it },
                openEpNodeId, openEp, onTabToMe,
                Modifier.fillMaxSize(),
            )
        }
    }
}

@Composable
private fun DirPane(
    state: AppState,
    courses: List<CourseUi>,
    openCourseId: Long?,
    onToggleCourse: (Long?) -> Unit,
    openEpNodeId: Long?,
    onOpenEp: (EpUi) -> Unit,
    onTabToMe: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier) {
        AppBar(
            title = state.workspace?.examGoal ?: "备考空间",
            subtitle = "备考空间 · AI 生成内容",
            trailing = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CountdownChip(state.daysLeft()) { state.goalSheet = true }
                    Avatar(
                        state.user?.nickname ?: state.user?.account ?: "学",
                        onClick = onTabToMe,
                    )
                }
            },
        )
        Column(
            Modifier
                .fillMaxWidth()
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp)
        ) {
            CountdownCard(
                days = state.daysLeft(),
                goal = state.workspace?.examGoal ?: "",
                date = state.examDate,
                onClick = { state.goalSheet = true },
            )
            SectionTitle("内容 · AI 生成（点目录展开）")
            if (courses.isEmpty()) {
                Text(
                    "暂无内容 · Agent 生成笔记后展示",
                    Modifier.fillMaxWidth().padding(vertical = 24.dp),
                    color = Xb.mutedLight, fontSize = 12.5.sp, textAlign = TextAlign.Center,
                )
            }
            courses.forEach { course ->
                CourseCard(
                    course,
                    open = openCourseId == course.dirId,
                    onToggle = { onToggleCourse(if (openCourseId == course.dirId) null else course.dirId) },
                    selNodeId = openEpNodeId,
                    onOpenEp = onOpenEp,
                )
            }
            SpacerPad(96.dp)
        }
    }
}

@Composable
private fun SpacerPad(h: androidx.compose.ui.unit.Dp) {
    Box(Modifier.height(h))
}

/** .countdown-card：渐变白紫卡片，点击修改目标/日期。 */
@Composable
private fun CountdownCard(days: Long, goal: String, date: String, onClick: () -> Unit) {
    val shape = RoundedCornerShape(14.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(Brush.linearGradient(listOf(Color.White, Color(0xFFF6F3FB))))
            .border(1.dp, Xb.borderLight, shape)
            .clickable(onClick = onClick)
            .padding(horizontal = 17.dp, vertical = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text("$days", fontFamily = FontFamily.Serif, fontSize = 34.sp, fontWeight = FontWeight.Bold, color = Xb.accentDeep)
        Text("天", fontFamily = FontFamily.Serif, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = Xb.accentDeep)
        Column(Modifier.weight(1f)) {
            Text("距离考试 · 点击修改目标/日期", fontSize = 11.5.sp, color = Xb.mutedLight)
            Text(
                goal.ifBlank { "未设置考试目标" },
                fontSize = 13.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink,
                maxLines = 1,
            )
            Text(date.ifBlank { "未设置考试日期" }, fontSize = 11.5.sp, color = Xb.muted)
        }
    }
}

/** .course：课程卡片，头部可展开集列表。 */
@Composable
private fun CourseCard(
    course: CourseUi,
    open: Boolean,
    onToggle: () -> Unit,
    selNodeId: Long?,
    onOpenEp: (EpUi) -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Xb.surface)
            .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .clickable(onClick = onToggle)
                .padding(horizontal = 14.dp, vertical = 13.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Box(
                Modifier.size(34.dp).clip(RoundedCornerShape(10.dp)).background(Xb.accentLight),
                contentAlignment = Alignment.Center,
            ) {
                Text("📚", fontSize = 16.sp)
            }
            Column(Modifier.weight(1f)) {
                Text(course.name, fontSize = 13.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink, maxLines = 1)
                Text("${course.episodes.size} 集 · AI 生成", fontSize = 11.sp, color = Xb.mutedLight)
            }
            Text("▶", fontSize = 12.sp, color = Xb.mutedLight, modifier = Modifier.rotate(if (open) 90f else 0f))
        }
        if (open) {
            Column(Modifier.fillMaxWidth().background(Xb.surface)) {
                course.episodes.forEachIndexed { _, ep ->
                    val sel = selNodeId == ep.nodeId
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .background(if (sel) Xb.accentLight else Color.Transparent)
                            .clickable { onOpenEp(ep) }
                            .padding(horizontal = 14.dp, vertical = 11.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(9.dp),
                    ) {
                        Text("第${ep.no}集", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Xb.mutedLight, modifier = Modifier.width(30.dp))
                        Text(ep.title, fontSize = 13.sp, fontWeight = FontWeight.Medium, color = Xb.ink, modifier = Modifier.weight(1f), maxLines = 1)
                        Badge("AI 生成")
                    }
                }
            }
        }
    }
}

/** .note-pane：笔记阅读区（头部栏 + 滚动内容）。 */
@Composable
private fun NotePane(state: AppState, modifier: Modifier = Modifier, showBack: Boolean, bottomPad: androidx.compose.ui.unit.Dp) {
    Column(modifier.background(Xb.surface)) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(start = 14.dp, end = 14.dp, top = 10.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (showBack) {
                Box(
                    Modifier
                        .clip(RoundedCornerShape(999.dp))
                        .background(Xb.surface2)
                        .clickable { state.closeNote() }
                        .padding(horizontal = 13.dp, vertical = 7.dp),
                ) {
                    Text("← 目录", fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold, color = Xb.ink)
                }
            }
            Text(
                state.currentItem?.item?.name ?: "选择一集查看笔记",
                fontSize = 14.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink,
                modifier = Modifier.weight(1f), maxLines = 1,
            )
            if (state.currentItem != null) {
                Badge("AI 生成")
                val annoOn = state.annoMode
                Box(
                    Modifier
                        .clip(RoundedCornerShape(999.dp))
                        .background(if (annoOn) Xb.accent else Xb.accentLight)
                        .clickable {
                            state.annoMode = !state.annoMode
                            if (state.annoMode) state.toast("批注模式：点击段落或卡片添加批注")
                        }
                        .padding(horizontal = 13.dp, vertical = 7.dp),
                ) {
                    Text(
                        if (annoOn) "✅ 完成" else "✏️ 批注",
                        fontSize = 12.5.sp, fontWeight = FontWeight.Bold,
                        color = if (annoOn) Color.White else Xb.accentDeep,
                    )
                }
            }
        }
        Box(Modifier.fillMaxWidth().weight(1f)) {
            val bundle = state.currentItem
            if (bundle == null) {
                Column(
                    Modifier.fillMaxSize(),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text("📖", fontSize = 40.sp, color = Xb.mutedLight.copy(alpha = 0.5f))
                    Text(
                        "从左侧目录选择一集\n查看 AI 生成的笔记",
                        color = Xb.mutedLight, fontSize = 13.sp, textAlign = TextAlign.Center,
                    )
                }
            } else {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .verticalScroll(rememberScrollState())
                        .padding(start = 18.dp, end = 18.dp, top = 16.dp)
                        .padding(bottom = bottomPad)
                ) {
                    if (state.annoMode) {
                        Text(
                            "✏️ 批注模式已开启：点击任意段落或知识卡片即可添加批注（再次点按钮退出）",
                            Modifier.fillMaxWidth().padding(bottom = 12.dp)
                                .clip(RoundedCornerShape(10.dp)).background(Xb.accentLight)
                                .padding(horizontal = 13.dp, vertical = 9.dp),
                            color = Xb.accentDeep, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, lineHeight = 19.sp,
                        )
                    }
                    Row(Modifier.fillMaxWidth().padding(bottom = 16.dp), horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                        Badge("AI 生成笔记")
                        Badge("B站课程 → AI 转写整理", bg = Xb.surface2, fg = Xb.muted)
                    }
                    NoteBody(
                        content = bundle.item.content ?: "",
                        annos = bundle.annotations,
                        annoMode = state.annoMode,
                        onAnnoTarget = { quote ->
                            state.annoQuote = quote.trim().take(42)
                            state.annoSheet = true
                        },
                        onAnnoClick = { anno -> state.annoDetail = anno },
                    )
                }
            }
        }
    }
}

/** 笔记 markdown 正文：标题 / 段落 / 知识卡片 / 列表 / 速查表 / 考试提示。 */
@Composable
private fun NoteBody(
    content: String,
    annos: List<Annotation>,
    annoMode: Boolean,
    onAnnoTarget: (String) -> Unit,
    onAnnoClick: (Annotation) -> Unit,
) {
    val blocks = remember(content) { parseMarkdown(content) }
    if (blocks.isEmpty()) {
        Text("该集暂无内容", color = Xb.mutedLight, fontSize = 13.sp, modifier = Modifier.padding(vertical = 20.dp))
        return
    }
    Column(Modifier.fillMaxWidth()) {
        for (block in blocks) {
            when (block) {
                is MdBlock.Heading -> {
                    Row(
                        Modifier.padding(top = 18.dp, bottom = 9.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Box(Modifier.size(width = 4.dp, height = 14.dp).clip(RoundedCornerShape(2.dp)).background(Xb.accent))
                        Text(block.content, fontSize = 13.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink)
                    }
                }
                is MdBlock.Para -> {
                    val plain = inlinePlain(block.content)
                    val kcard =
                        if (annoChunks(block.content, annos).all { it.second == null }) kcardParts(block.content)
                        else null
                    if (kcard != null) {
                        // .kcard：surface-2 卡片 + 粗体标题
                        val shape = RoundedCornerShape(12.dp)
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .clip(shape)
                                .background(Xb.surface2)
                                .border(1.dp, Xb.borderLight, shape)
                                .then(annoTargetBorder(annoMode, onClick = { onAnnoTarget(plain) }))
                                .padding(horizontal = 14.dp, vertical = 12.dp),
                        ) {
                            Text(kcard.first, fontSize = 13.sp, fontWeight = FontWeight.Bold, color = Xb.ink, modifier = Modifier.padding(bottom = 4.dp))
                            Text(kcard.second, fontSize = 12.5.sp, color = Xb.muted, lineHeight = 22.sp)
                        }
                    } else {
                        Text(
                            renderInlineAnnotated(block.content, annos, onAnnoClick),
                            fontSize = 13.5.sp, color = Xb.ink, lineHeight = 26.sp,
                            modifier = Modifier
                                .fillMaxWidth()
                                .then(annoTargetBorder(annoMode, onClick = { onAnnoTarget(plain) }))
                                .padding(bottom = 10.dp),
                        )
                    }
                }
                is MdBlock.List -> {
                    Column(Modifier.fillMaxWidth().padding(bottom = 10.dp)) {
                        block.items.forEachIndexed { idx, item ->
                            Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                                Text(
                                    if (block.ordered) "${idx + 1}. " else "•  ",
                                    fontSize = 13.5.sp, color = Xb.muted, fontWeight = FontWeight.SemiBold,
                                    modifier = Modifier.width(if (block.ordered) 30.dp else 18.dp),
                                )
                                Text(
                                    renderInlineAnnotated(item, annos, onAnnoClick),
                                    fontSize = 13.5.sp, color = Xb.ink, lineHeight = 24.sp,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                }
                is MdBlock.Table -> {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(bottom = 10.dp)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Xb.surface)
                            .border(1.dp, Xb.borderLight, RoundedCornerShape(10.dp))
                    ) {
                        for (rIdx in block.rows.indices) {
                            val row = block.rows[rIdx]
                            Row(Modifier.fillMaxWidth().background(if (rIdx == 0) Xb.surface2 else Color.Transparent)) {
                                for (cIdx in row.indices) {
                                    Text(
                                        renderInlineAnnotated(row[cIdx], annos, onAnnoClick),
                                        fontSize = if (rIdx == 0) 11.5.sp else 12.sp,
                                        color = if (rIdx == 0) Xb.muted else Xb.ink,
                                        lineHeight = 19.sp,
                                        modifier = Modifier
                                            .weight(1f)
                                            .padding(horizontal = 10.dp, vertical = 8.dp),
                                    )
                                }
                            }
                            if (rIdx == 0 && block.rows.size > 1) {
                                Box(Modifier.fillMaxWidth().height(1.dp).background(Xb.borderLight))
                            }
                        }
                    }
                }
                is MdBlock.Tip -> {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(bottom = 10.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(Xb.goldLight)
                            .border(1.dp, Color(0xFFEBD9AE), RoundedCornerShape(12.dp))
                            .padding(horizontal = 14.dp, vertical = 12.dp),
                    ) {
                        Text(
                            block.lines.joinToString("\n"),
                            fontSize = 12.5.sp, color = Color(0xFF7A5B1E), lineHeight = 22.sp,
                        )
                    }
                }
            }
        }
    }
}

/** .anno-target：批注模式下段落/卡片虚线轮廓。 */
@Composable
private fun annoTargetBorder(annoMode: Boolean, onClick: () -> Unit): Modifier {
    if (!annoMode) return Modifier
    return Modifier
        .clip(RoundedCornerShape(6.dp))
        .clickable(onClick = onClick)
        .drawBehind {
            drawRoundRect(
                color = Xb.accent,
                style = Stroke(
                    width = 1.5.dp.toPx(),
                    pathEffect = PathEffect.dashPathEffect(floatArrayOf(6f, 4f)),
                ),
            )
        }
}

// ==================== Sheet：考试目标 / 补充批注 / 批注详情 ====================

/** sheetGoal：考试目标设置（手写填写 · 倒计时同步更新）。 */
@Composable
fun GoalSheet(state: AppState) {
    XbSheet(
        open = state.goalSheet,
        onDismiss = { state.goalSheet = false },
        title = "🎯 考试目标设置",
        subtitle = "手写填写 · 倒计时同步更新",
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp)) {
            FormRow("考试目标（手写填写）") {
                FormInput(state.examGoal, { state.examGoal = it }, placeholder = "如：软考 · 系统架构设计师")
            }
            FormRow("考试日期") {
                FormInput(state.examDate, { state.examDate = it }, placeholder = "2026-11-07")
            }
            XbButton("保存设置", onClick = {
                if (state.saveGoal(state.examGoal, state.examDate)) state.goalSheet = false
            })
        }
    }
}

/** sheetAnno：补充批注（引文 + 内容）。 */
@Composable
fun AnnoSheet(state: AppState) {
    XbSheet(
        open = state.annoSheet,
        onDismiss = { state.annoSheet = false },
        title = "✏️ 补充批注",
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp)) {
            val quote = state.annoQuote
            Text(
                "「$quote${if (quote.length >= 42) "…" else ""}」",
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Xb.surface2)
                    .border(
                        BorderStroke(1.dp, Xb.gold.copy(alpha = 0.5f)),
                        RoundedCornerShape(topStart = 9.dp, bottomStart = 9.dp, topEnd = 9.dp, bottomEnd = 9.dp),
                    )
                    .padding(horizontal = 13.dp, vertical = 9.dp),
                color = Xb.ink, fontSize = 13.sp, lineHeight = 21.sp,
            )
            SpacerPad(14.dp)
            BasicTextField(
                value = state.annoText,
                onValueChange = { state.annoText = it },
                textStyle = androidx.compose.ui.text.TextStyle(
                    fontSize = 14.sp, color = Xb.ink, lineHeight = 21.sp,
                ),
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 110.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(Xb.surface)
                    .border(1.dp, Xb.border, RoundedCornerShape(10.dp))
                    .padding(horizontal = 13.dp, vertical = 11.dp),
                decorationBox = { inner ->
                    Box {
                        if (state.annoText.isEmpty()) {
                            Text("输入批注内容…", color = Xb.mutedLight, fontSize = 14.sp)
                        }
                        inner()
                    }
                },
            )
            SpacerPad(16.dp)
            XbButton("保存批注", onClick = {
                val text = state.annoText.trim()
                if (text.isEmpty()) {
                    state.toast("批注内容不能为空")
                    return@XbButton
                }
                state.annoText = ""
                state.annoSheet = false
                state.annoMode = false
                state.saveAnnotation(text)
            })
        }
    }
}

/** sheetAnnoDetail：批注详情（AI 批注 → 让 AI 解释；我的批注 → 删除）。 */
@Composable
fun AnnoDetailSheet(state: AppState) {
    val anno = state.annoDetail ?: return
    val mine = anno.author == AnnotationAuthor.User
    XbSheet(
        open = true,
        onDismiss = { state.annoDetail = null },
        title = "📌 批注详情",
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp)) {
            Text("批注位置", color = Xb.muted, fontSize = 12.sp, modifier = Modifier.padding(bottom = 6.dp))
            val quote = anno.anchor
            Text(
                "「$quote${if (quote.length >= 42) "…" else ""}」",
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Xb.surface2)
                    .border(
                        BorderStroke(1.dp, Xb.gold.copy(alpha = 0.5f)),
                        RoundedCornerShape(topStart = 9.dp, bottomStart = 9.dp, topEnd = 9.dp, bottomEnd = 9.dp),
                    )
                    .padding(horizontal = 13.dp, vertical = 9.dp),
                color = Xb.ink, fontSize = 13.sp, lineHeight = 21.sp,
            )
            SpacerPad(14.dp)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("批注内容", color = Xb.muted, fontSize = 12.sp)
                if (mine) Badge("我的批注", bg = Xb.accentLight, fg = Xb.accentDeep)
                else Badge("AI 批注（老师强调/考点）")
            }
            SpacerPad(8.dp)
            Text(anno.text, color = Xb.ink, fontSize = 13.5.sp, lineHeight = 24.sp, modifier = Modifier.padding(bottom = 16.dp))
            if (mine) {
                XbButton(
                    "🗑 删除这条批注",
                    onClick = {
                        state.annoDetail = null
                        state.deleteAnnotation(anno.id)
                    },
                    primary = false,
                    textColor = Xb.red,
                )
            } else {
                XbButton("🤖 让 AI 解释", onClick = {
                    state.annoDetail = null
                    state.toast("已发送给 Agent 请求解答（演示）")
                })
            }
        }
    }
}

/** 渲染一行内联 markdown（无批注），供题目解析 / 说明文本复用。 */
@Composable
internal fun MdInlineText(
    src: String,
    color: androidx.compose.ui.graphics.Color,
    fontSize: androidx.compose.ui.unit.TextUnit,
    lineHeight: androidx.compose.ui.unit.TextUnit,
    modifier: Modifier = Modifier,
) {
    Text(
        renderInlineAnnotated(src, emptyList(), {}),
        color = color, fontSize = fontSize, lineHeight = lineHeight,
        modifier = modifier,
    )
}
