package com.xueban.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** 组卷页：来源 / 题型 / 范围 / 数量筛选 + 预览与模考入口，与原型 pageAssembly 对应。 */
@Composable
fun AssemblyScreen(state: AppState, onOpenGoal: () -> Unit) {
    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
        AppBar(
            title = "组卷",
            subtitle = "从题库筛选题目 · 组装模拟试卷",
            trailing = { CountdownChip(state.daysLeft(), onClick = onOpenGoal) },
        )
        Column(Modifier.padding(horizontal = 16.dp)) {
            SrcHint("💡 组卷数据来自题库：AI 生成的习题 + 你的错题，按条件筛选组合成模拟试卷")

            // §12.5：展开态（≥600dp）组卷双列——左 来源/题型/范围，右 数量 + 操作（原型 .fold-open .asm-layout）
            BoxWithConstraints(Modifier.fillMaxWidth()) {
                if (maxWidth >= 600.dp) {
                    Row(
                        Modifier.fillMaxWidth().padding(top = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Column(Modifier.weight(1f)) {
                            AsmSourceCard(state)
                            AsmTypeCard(state)
                            AsmScopeCard(state)
                        }
                        Column(Modifier.weight(1f)) {
                            AsmCountCard(state)
                            AsmButtons(state)
                        }
                    }
                } else {
                    Column(Modifier.padding(top = 12.dp)) {
                        AsmSourceCard(state)
                        AsmTypeCard(state)
                        AsmScopeCard(state)
                        AsmCountCard(state)
                        AsmButtons(state)
                    }
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }

    if (state.previewSheet) {
        PreviewSheet(state = state, onStartMock = { state.startMock(state.previewPaper!!) })
    }
}

@Composable
private fun AsmSection(title: String, content: @Composable () -> Unit) {
    Text(title, color = Xb.ink, fontSize = 13.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(bottom = 10.dp, top = 4.dp))
    content()
}

/** .asm-card：白底卡片 + 细边框（原型组卷页四个筛选卡片 + 操作区的公共容器）。 */
@Composable
private fun AsmCard(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(bottom = 12.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(Xb.surface)
            .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
            .padding(start = 15.dp, end = 15.dp, top = 15.dp, bottom = 17.dp),
        content = content,
    )
}

@Composable
private fun AsmSourceCard(state: AppState) {
    AsmCard {
        AsmSection("题目来源") {
            ChipRow(listOf("全部", "AI 生成题", "错题"), state.asmSource) {
                state.asmSource = it
            }
        }
    }
}

@Composable
private fun AsmTypeCard(state: AppState) {
    AsmCard {
        AsmSection("题型") {
            ChipRow(listOf("单选题", "多选题", "判断题"), state.asmType) {
                state.asmType = it
            }
        }
    }
}

@Composable
private fun AsmScopeCard(state: AppState) {
    AsmCard {
        AsmSection("范围") {
            val eps = allEps(deriveCourses(state.tree))
            // 标签带课程名，区分不同课程下的同号集（如「软考 · 第1集」），
            // 避免多课程时「第1集」重复，且点击按 no 匹配会错选第一个同号集。
            val labels = listOf("全部集数") + eps.map { "${it.first.name} · 第${it.second.no}集" }
            ChipRow(labels, state.asmScope) { label ->
                val ep = eps.firstOrNull { "${it.first.name} · 第${it.second.no}集" == label }?.second
                if (ep == null) {
                    state.asmScope = "全部集数"
                    state.asmScopeId = null
                } else {
                    state.asmScope = label
                    state.asmScopeId = ep.nodeId
                }
            }
        }
    }
}

@Composable
private fun AsmCountCard(state: AppState) {
    AsmCard {
        AsmSection("题目数量（综合知识真题为 75 题）") {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                StepperBtn("−") {
                    state.asmCount = (state.asmCount - 5).coerceAtLeast(5)
                }
                Text(
                    "${state.asmCount}",
                    color = Xb.ink, fontSize = 20.sp, fontWeight = FontWeight.Bold,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.width(74.dp),
                )
                Text("题", color = Xb.mutedLight, fontSize = 11.5.sp)
                StepperBtn("＋") {
                    state.asmCount = (state.asmCount + 5).coerceAtMost(150)
                }
            }
        }
    }
}

@Composable
private fun AsmButtons(state: AppState) {
    Column(Modifier.padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        XbButton("⚡ 自动补齐到 75 题", onClick = {
            state.asmCount = 75
            state.toast("已自动补齐到 75 题：AI 生成题优先，不足部分用错题补足")
        }, modifier = Modifier.fillMaxWidth(), primary = false)
        XbButton("👁 预览试卷", onClick = {
            state.assemble()
            if (state.previewPaper != null) state.previewSheet = true
        }, modifier = Modifier.fillMaxWidth(), primary = false)
        XbButton("🚀 开始模考", onClick = {
            state.assemble()
            state.previewPaper?.let { state.startMock(it) }
        }, modifier = Modifier.fillMaxWidth())
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ChipRow(options: List<String>, selected: String, onSelect: (String) -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        options.forEach { opt ->
            val on = opt == selected
            Text(
                opt,
                modifier = Modifier
                    .clip(RoundedCornerShape(999.dp))
                    .background(if (on) Xb.accentLight else Xb.surface2)
                    .border(1.5.dp, if (on) Xb.accent else Color.Transparent, RoundedCornerShape(999.dp))
                    .clickable { onSelect(opt) }
                    .padding(horizontal = 14.dp, vertical = 7.dp),
                color = if (on) Xb.accentDeep else Xb.muted,
                fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold,
            )
        }
    }
}

@Composable
private fun StepperBtn(symbol: String, onClick: () -> Unit) {
    Box(
        Modifier
            .size(38.dp)
            .clip(RoundedCornerShape(11.dp))
            .background(Xb.surface2)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(symbol, color = Xb.muted, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
    }
}

/** 试卷预览弹层：构成比例 + 题库可用量 + 样题。 */
@Composable
fun PreviewSheet(state: AppState, onStartMock: () -> Unit) {
    val bundle = state.previewPaper
    if (bundle == null) return
    val n = bundle.questions.size
    val singles = (n * 0.8).toInt().coerceAtLeast(0)
    val multis = (n * 0.15).toInt().coerceAtLeast(0)
    val judges = (n - singles - multis).coerceAtLeast(0)

    var bankCount by remember { mutableIntStateOf(0) }
    LaunchedEffect(state.asmScopeId) {
        bankCount = if (state.asmScope == "全部集数") {
            fetchScopeCount(state, null) + state.wrongList.size
        } else {
            fetchScopeCount(state, state.asmScopeId)
        }
    }

    XbSheet(
        open = true,
        onDismiss = { state.previewSheet = false },
        title = "👁 试卷预览",
        subtitle = "${state.asmSource} · ${state.asmType} · ${state.asmScope} · 共 $n 题",
    ) {
        Column(Modifier.padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
            var first = true
            ResultRow(label = "单选题", value = "$singles 题", first)
            first = false
            ResultRow(label = "多选题", value = "$multis 题", first)
            ResultRow(label = "判断题", value = "$judges 题", first)
            ResultRow(label = "题库可用（当前筛选）", value = "$bankCount 题", first)
            SrcHint("💡 题目将从题库（AI 生成题 + 错题）中按条件抽取；样题预览如下", Modifier.padding(top = 12.dp))
            bundle.questions.take(2).forEach { q ->
                Column(
                    Modifier
                        .fillMaxWidth()
                        .padding(top = 10.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(Xb.surface2)
                        .border(1.dp, Xb.borderLight, RoundedCornerShape(12.dp))
                        .padding(horizontal = 12.dp, vertical = 14.dp)
                ) {
                    Text(q.stem, color = Xb.ink, fontSize = 13.sp, fontWeight = FontWeight.Bold, lineHeight = 21.sp)
                    Text("EP · AI 生成", color = Xb.muted, fontSize = 12.5.sp, modifier = Modifier.padding(top = 5.dp))
                }
            }
            XbButton("直接开始模考", onClick = {
                state.previewSheet = false
                onStartMock()
            }, modifier = Modifier.fillMaxWidth().padding(top = 14.dp))
            Spacer(Modifier.height(10.dp))
        }
    }
}

@Composable
private fun ResultRow(label: String, value: String, first: Boolean) {
    Column(Modifier.fillMaxWidth()) {
        if (!first) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(Xb.borderLight))
        }
        androidx.compose.foundation.layout.Row(
            Modifier.fillMaxWidth().padding(horizontal = 2.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(label, color = Xb.muted, fontSize = 13.sp)
            Text(value, color = Xb.ink, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}
