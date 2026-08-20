package com.xueban.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** 刷题页：范围切换 + AI 习题作答，与原型 pageQuiz 对应。 */
@Composable
fun QuizScreen(state: AppState, onOpenGoal: () -> Unit) {
    var pickerOpen by remember { mutableStateOf(false) }
    var scopeCount by remember { mutableIntStateOf(0) }

    LaunchedEffect(Unit) {
        if (state.quizPool.isEmpty()) state.loadQuiz()
        scopeCount = fetchScopeCount(state, state.quizScopeId)
    }

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
        AppBar(
            title = "刷题",
            subtitle = "题库来自 AI 为每集笔记生成的习题",
            trailing = { CountdownChip(state.daysLeft(), onClick = onOpenGoal) },
        )
        // §12.5：展开态（≥600dp）题卡限宽 620dp 居中，避免行长过长（原型 .fold-open .q-wrap）
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val wide = maxWidth >= 600.dp
            Column(
                Modifier
                    .align(Alignment.TopCenter)
                    .padding(horizontal = 16.dp)
                    .then(if (wide) Modifier.widthIn(max = 620.dp) else Modifier)
            ) {
                // scope-bar
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(14.dp))
                        .background(Xb.surface)
                        .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
                        .clickable { pickerOpen = true }
                        .padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("刷题范围 · 点击切换", color = Xb.mutedLight, fontSize = 11.sp)
                        Text(
                            state.quizScope,
                            color = Xb.ink, fontSize = 13.5.sp, fontWeight = FontWeight.Bold,
                            modifier = Modifier.padding(top = 2.dp),
                        )
                    }
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(999.dp))
                            .background(Xb.accentLight)
                            .padding(horizontal = 9.dp, vertical = 3.dp)
                    ) {
                        Text("$scopeCount 题", color = Xb.accentDeep, fontSize = 11.5.sp, fontWeight = FontWeight.Bold)
                    }
                }
                SrcHint("💡 题库 = AI 为每集笔记生成的习题。例如「第1集 软件架构概念」生成的习题，就是该集范围内的题目", Modifier.padding(top = 12.dp))

                // q-progress
                Row(Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (state.quizPool.isEmpty()) "第 0 题" else "第 ${state.quizIdx + 1} / ${state.quizPool.size} 题",
                        color = Xb.muted, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.weight(1f),
                    )
                    Badge("对 ${state.quizRight}", Xb.greenLight, Xb.green)
                    Spacer(Modifier.size(6.dp))
                    Badge("错 ${state.quizWrong}", Xb.redLight, Xb.red)
                }

                val q = state.currentQuestion()
                if (q == null) {
                    Column(Modifier.fillMaxWidth().padding(vertical = 30.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            "该范围暂无题目\n（AI 生成该集习题后即可刷题）",
                            color = Xb.mutedLight, fontSize = 13.sp, lineHeight = 22.sp,
                            modifier = Modifier.align(Alignment.CenterHorizontally),
                        )
                    }
                } else {
                    QuizCard(state, q, onOpenGoal)
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }

    if (pickerOpen) {
        EpSheet(
            state = state,
            courses = deriveCourses(state.tree),
            onPick = { scopeId, name ->
                state.loadQuiz(scopeId, name)
                scopeCount = fetchScopeCount(state, scopeId)
                state.toast("刷题范围：$name")
            },
            onDismiss = { pickerOpen = false },
        )
    }
}

@Composable
private fun QuizCard(state: AppState, q: QuestionBrief, onOpenGoal: () -> Unit) {
    var multiSel by remember(q.id) { mutableStateOf(emptyList<Int>()) }
    val outcome = state.quizOutcome
    val corrects = if (outcome != null) correctIndexSet(q, outcome) else emptySet()

    // 视频题：独立作答卡片（上传视频 + 训练想法，不判分）。
    if (q.qtype == QuestionType.Video) {
        VideoAnswerCard(state, q)
        return
    }

    XbCard {
        androidx.compose.foundation.layout.Column(Modifier.padding(16.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                val (bg, fg, label) = badgeAi()
                Badge(label, bg, fg)
                Badge(state.epNameOf(q.sourceItemId), Xb.surface2, Xb.muted)
            }
            Text(
                q.stem,
                color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp,
                modifier = Modifier.padding(top = 11.dp, bottom = 13.dp),
            )
            q.options.forEachIndexed { i, opt ->
                val key = ('A' + i).toChar()
                val accent = when {
                    i in corrects -> Xb.green
                    q.qtype == QuestionType.Multi && i in multiSel -> Xb.red
                    q.qtype != QuestionType.Multi && i == state.quizPicked -> Xb.red
                    else -> null
                }
                val dimmed = accent == null
                QOption(
                    key = key,
                    text = opt,
                    accent = accent,
                    dimmed = dimmed,
                    modifier = Modifier.padding(bottom = 9.dp),
                    onClick = {
                        if (state.quizAnswered) return@QOption
                        if (q.qtype == QuestionType.Multi) {
                            multiSel = if (i in multiSel) multiSel - i else multiSel + i
                        } else {
                            state.submitAnswer(i)
                        }
                    },
                )
            }
            if (q.qtype == QuestionType.Multi && !state.quizAnswered) {
                XbButton(
                    "确认答案（已选 ${multiSel.size} 项）",
                    onClick = {
                        if (multiSel.isEmpty()) {
                            state.toast("请先选择至少一项")
                            return@XbButton
                        }
                        state.submitAnswerMulti(multiSel)
                    },
                    modifier = Modifier.fillMaxWidth(),
                    primary = false,
                )
            }
            if (state.quizAnswered && outcome != null) {
                val exp = outcome.explanation
                if (!exp.isNullOrBlank()) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = 13.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(Xb.surface2)
                            .padding(horizontal = 14.dp, vertical = 12.dp)
                    ) {
                        MdInlineText(exp, Xb.muted, 12.5.sp, 22.sp)
                    }
                }
                val isLast = state.quizIdx >= state.quizPool.size - 1
                XbButton(
                    if (isLast) "完成本轮 ✓" else "下一题 →",
                    onClick = {
                        if (isLast) {
                            state.toast("本轮刷题完成 · 对 ${state.quizRight} / 错 ${state.quizWrong}")
                            state.loadQuiz(state.quizScopeId, state.quizScope)
                        } else {
                            state.nextQuestion()
                        }
                        multiSel = emptyList()
                    },
                    modifier = Modifier.fillMaxWidth().padding(top = 13.dp),
                )
            }
        }
    }
}

/** 视频作答题卡片：上传训练视频/图片 + 训练想法，不判分，提交待 AI 复盘。 */
@Composable
private fun VideoAnswerCard(state: AppState, q: QuestionBrief) {
    val context = LocalContext.current
    var files by remember(q.id) { mutableStateOf<List<Pair<String, ByteArray>>>(emptyList()) }
    var note by remember(q.id) { mutableStateOf("") }
    var uploading by remember(q.id) { mutableStateOf(false) }
    val submitted = q.id in state.videoSubmitted

    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments()
    ) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        uploading = true
        val picked = uris.mapNotNull { uri ->
            runCatching {
                val name = queryDisplayName(context, uri) ?: "训练视频"
                val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                if (bytes == null) null else name to bytes
            }.getOrNull()
        }
        files = files + picked
        uploading = false
    }

    XbCard {
        Column(Modifier.padding(16.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                Badge("视频作答", Xb.accentLight, Xb.accentDeep)
                Badge("不判对错 · 交给 AI 复盘", Xb.surface2, Xb.muted)
            }
            Text(
                q.stem,
                color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp,
                modifier = Modifier.padding(top = 11.dp, bottom = 13.dp),
            )
            if (submitted) {
                Text(
                    "✓ 已提交 · 等待 AI 复盘",
                    color = Xb.green, fontSize = 13.sp, fontWeight = FontWeight.Bold,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(999.dp))
                        .background(Xb.greenLight)
                        .padding(horizontal = 13.dp, vertical = 9.dp),
                )
            } else {
                XbButton(
                    if (uploading) "读取中…" else "🎬 选择视频 / 图片",
                    onClick = { launcher.launch(arrayOf("video/*", "image/*")) },
                    modifier = Modifier.fillMaxWidth(),
                    primary = false,
                    enabled = !uploading,
                )
                files.forEach { (name, bytes) ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = 9.dp)
                            .clip(RoundedCornerShape(10.dp))
                            .background(Xb.surface2)
                            .padding(horizontal = 12.dp, vertical = 9.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            "$name · ${bytes.size / 1024}KB",
                            color = Xb.muted, fontSize = 12.5.sp,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            "✕",
                            color = Xb.mutedLight, fontSize = 13.sp,
                            modifier = Modifier
                                .clip(RoundedCornerShape(6.dp))
                                .clickable { files = files - (name to bytes) }
                                .padding(horizontal = 6.dp, vertical = 2.dp),
                        )
                    }
                }
                Text(
                    "训练想法（可选 · 让 AI 更懂你的问题）",
                    color = Xb.muted, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(top = 14.dp, bottom = 6.dp),
                )
                androidx.compose.foundation.text.BasicTextField(
                    value = note,
                    onValueChange = { note = it },
                    textStyle = androidx.compose.ui.text.TextStyle(fontSize = 13.5.sp, color = Xb.ink),
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(10.dp))
                        .background(Xb.surface2)
                        .padding(horizontal = 13.dp, vertical = 11.dp),
                    decorationBox = { inner ->
                        Box {
                            if (note.isEmpty()) {
                                Text("如：发力总用不上腰，重心前冲…", color = Xb.mutedLight, fontSize = 13.5.sp)
                            }
                            inner()
                        }
                    },
                )
                XbButton(
                    "提交给 AI 复盘",
                    onClick = {
                        if (files.isEmpty()) {
                            state.toast("请先选择训练视频或图片")
                            return@XbButton
                        }
                        state.submitVideo(q.id, q.sourceItemId, files, note)
                    },
                    modifier = Modifier.fillMaxWidth().padding(top = 13.dp),
                )
            }
        }
    }
}

private fun queryDisplayName(context: android.content.Context, uri: android.net.Uri): String? {
    return runCatching {
        context.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()
}

/** 选择刷题范围弹层：搜索 + 课程分组 + 每集题数。 */
@Composable
fun EpSheet(
    state: AppState,
    courses: List<CourseUi>,
    onPick: (scopeId: Long?, name: String) -> Unit,
    onDismiss: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var counts by remember { mutableStateOf<Map<Long, Int>>(emptyMap()) }
    var loaded by remember { mutableStateOf(false) }
    var allCount by remember { mutableIntStateOf(0) }

    LaunchedEffect(Unit) {
        allCount = fetchScopeCount(state, null)
        counts = deriveCourses(state.tree).flatMap { it.episodes }
            .associate { it.nodeId to fetchScopeCount(state, it.nodeId) }
        loaded = true
    }

    val q = query.trim()
    XbSheet(
        open = true,
        onDismiss = onDismiss,
        title = "选择刷题范围",
        subtitle = "题库按集归属 · 来自 AI 生成的习题",
    ) {
        Column(Modifier.padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(999.dp))
                    .background(Xb.surface2)
                    .border(1.dp, Xb.border, RoundedCornerShape(999.dp))
                    .padding(horizontal = 14.dp, vertical = 10.dp)
            ) {
                if (query.isEmpty()) {
                    Text("🔍 搜索集数，如：架构风格", color = Xb.mutedLight, fontSize = 13.5.sp)
                }
                androidx.compose.foundation.text.BasicTextField(
                    value = query,
                    onValueChange = { query = it },
                    textStyle = androidx.compose.ui.text.TextStyle(fontSize = 13.5.sp, color = Xb.ink),
                )
            }
            Spacer(Modifier.height(6.dp))

            // 全部范围
            PickItem(
                title = "🗂 全部范围",
                selected = state.quizScope == "全部范围",
                count = if (loaded) "$allCount 题" else "…",
                onClick = { onPick(null, "全部范围") },
            )
            Spacer(Modifier.height(5.dp))

            for (course in courses) {
                val eps = course.episodes.filter { ep ->
                    q.isEmpty() || ep.title.contains(q) || "第${ep.no}集".contains(q)
                }
                if (eps.isEmpty()) continue
                Text(
                    course.name,
                    color = Xb.mutedLight, fontSize = 11.5.sp, fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 2.dp, top = 10.dp, bottom = 6.dp),
                )
                eps.forEach { ep ->
                    PickItem(
                        title = "第${ep.no}集 ${ep.title}",
                        selected = state.quizScopeId == ep.nodeId,
                        count = if (loaded) "${counts[ep.nodeId] ?: 0} 题" else "…",
                        onClick = { onPick(ep.nodeId, "第${ep.no}集 ${ep.title}") },
                    )
                    Spacer(Modifier.height(5.dp))
                }
            }
            Spacer(Modifier.height(14.dp))
        }
    }
}

@Composable
private fun PickItem(title: String, selected: Boolean, count: String, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(11.dp))
            .background(if (selected) Xb.accentLight else Xb.surface)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            color = Xb.ink, fontSize = 13.5.sp,
            fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
            modifier = Modifier.weight(1f),
        )
        Text(if (selected) "✓ $count" else count, color = Xb.mutedLight, fontSize = 11.5.sp)
    }
}

/** 静默统计某范围题数（失败按 0，不弹 toast）。 */
internal fun fetchScopeCount(state: AppState, scopeId: Long?): Int = runCatching {
    Api.draw(state.workspace?.id ?: return 0, scope = scopeId, count = 100).size
}.getOrElse { 0 }

/** 由 wire 格式的正确选项推导正确索引集（single→数字、multi→索引数组、judge→布尔）。 */
internal fun correctIndexSet(q: QuestionBrief, outcome: AnswerOutcome): Set<Int> = when (q.qtype) {
    QuestionType.Multi -> (outcome.answer as? JsonArray)
        ?.mapNotNull { it.jsonPrimitive.intOrNull }
        ?.toSet() ?: emptySet()
    QuestionType.Judge -> {
        val isTrue = runCatching { outcome.answer.jsonPrimitive.content.toBooleanStrictOrNull() }.getOrNull() ?: false
        val idx = if (isTrue) q.options.indexOf("正确") else q.options.indexOf("错误")
        if (idx >= 0) setOf(idx) else emptySet()
    }
    else -> setOfNotNull(outcome.answer.jsonPrimitive.intOrNull).filter { it in q.options.indices }.toSet()
}
