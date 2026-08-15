package com.xueban.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val MasteredBorder = Color(0xFFBFE0CE)

/** 错题本页：错题网格 + 掌握标记，与原型 pageWrong 对应。 */
@Composable
fun WrongScreen(state: AppState) {
    LaunchedEffect(Unit) {
        if (state.wrongList.isEmpty()) state.loadWrong()
    }

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
        AppBar(
            title = "错题本",
            subtitle = "刷题 / 模考答错自动归集",
            trailing = { CountdownChip(state.daysLeft()) },
        )
        Column(Modifier.padding(horizontal = 16.dp)) {
            SrcHint("💡 错题本数据来自使用过程：刷题 / 模考中的答错记录自动归集于此")
            if (state.wrongList.isEmpty()) {
                Text(
                    "🎉 暂无错题",
                    color = Xb.mutedLight, fontSize = 13.sp,
                    modifier = Modifier.fillMaxWidth().padding(vertical = 40.dp),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            } else {
                BoxWithConstraints {
                    val twoCol = maxWidth >= 600.dp
                    val rows = if (twoCol) state.wrongList.chunked(2) else state.wrongList.map { listOf(it) }
                    rows.forEach { rowItems ->
                        Row(
                            Modifier.fillMaxWidth().padding(top = 12.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            rowItems.forEach { item ->
                                Box(Modifier.weight(1f)) {
                                    WrongCard(state, item, state.wrongList.indexOf(item))
                                }
                            }
                            if (rowItems.size < 2 && twoCol) Spacer(Modifier.weight(1f))
                        }
                    }
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun WrongCard(state: AppState, item: WrongListItem, index: Int) {
    val mastered = item.wrong.mastered
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(if (mastered) Xb.greenLight else Xb.surface)
            .border(1.dp, if (mastered) MasteredBorder else Xb.borderLight, RoundedCornerShape(14.dp))
            .padding(13.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(9.dp))
                    .background(if (mastered) Xb.green else Xb.redLight),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    if (mastered) "✓" else "✗",
                    color = if (mastered) Color.White else Xb.red,
                    fontSize = 14.sp, fontWeight = FontWeight.Bold,
                )
            }
            Spacer(Modifier.size(10.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    item.question.stem,
                    color = Xb.ink, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, lineHeight = 21.sp,
                    maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
                Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(state.epNameOf(item.question.sourceItemId), color = Xb.mutedLight, fontSize = 11.sp)
                    Text("错 ${item.wrong.times} 次", color = Xb.mutedLight, fontSize = 11.sp)
                    if (mastered) {
                        Text("已掌握", color = Xb.green, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                    }
                }
            }
        }
        Row(Modifier.padding(top = 11.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            XbButton("重做", onClick = { state.redoWrong(index) }, small = true, modifier = Modifier.weight(1f))
            XbButton(
                if (mastered) "取消" else "掌握",
                onClick = { state.toggleMastered(index) },
                small = true,
                primary = false,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

/** 错题重做全屏覆盖层。 */
@Composable
fun RedoOverlay(state: AppState, onClose: () -> Unit) {
    val item = state.wrongList.getOrNull(state.redoIdx)
    Column(Modifier.fillMaxSize().background(Xb.bg)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                "✕ 关闭",
                modifier = Modifier
                    .clip(RoundedCornerShape(999.dp))
                    .background(Xb.surface2)
                    .clickable(onClick = onClose)
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                color = Xb.muted, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
            )
            Text(
                "错题重做",
                color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.Bold,
                modifier = Modifier.weight(1f),
            )
            Badge("仅本题", Xb.redLight, Xb.red)
        }
        if (item == null) {
            Text(
                "错题已清空",
                color = Xb.mutedLight, fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(vertical = 40.dp),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        } else {
            RedoCard(state, item, onClose)
        }
    }
}

@Composable
private fun RedoCard(state: AppState, item: WrongListItem, onClose: () -> Unit) {
    var multiSel by remember(item.question.id) { mutableStateOf(emptyList<Int>()) }
    val q = item.question
    val outcome = state.redoOutcome
    val corrects = if (outcome != null) correctIndexSet(q, outcome) else emptySet()

    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).verticalScroll(rememberScrollState())) {
        XbCard {
            androidx.compose.foundation.layout.Column(Modifier.padding(16.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    Badge("错题重做", Xb.redLight, Xb.red)
                    Badge(state.epNameOf(q.sourceItemId), Xb.surface2, Xb.muted)
                }
                Text(
                    q.stem,
                    color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp,
                    modifier = Modifier.padding(top = 11.dp, bottom = 13.dp),
                )
                q.options.forEachIndexed { i, opt ->
                    val accent = when {
                        i in corrects -> Xb.green
                        q.qtype == QuestionType.Multi && i in multiSel -> Xb.red
                        q.qtype != QuestionType.Multi && i == state.redoPicked -> Xb.red
                        else -> null
                    }
                    QOption(
                        key = ('A' + i).toChar(),
                        text = opt,
                        accent = accent,
                        dimmed = accent == null,
                        modifier = Modifier.padding(bottom = 9.dp),
                        onClick = {
                            if (state.redoAnswered) return@QOption
                            if (q.qtype == QuestionType.Multi) {
                                multiSel = if (i in multiSel) multiSel - i else multiSel + i
                            } else {
                                state.submitRedo(i)
                            }
                        },
                    )
                }
                if (q.qtype == QuestionType.Multi && !state.redoAnswered) {
                    XbButton(
                        "确认答案（已选 ${multiSel.size} 项）",
                        onClick = {
                            if (multiSel.isEmpty()) {
                                state.toast("请先选择至少一项")
                                return@XbButton
                            }
                            state.submitRedoMulti(multiSel)
                        },
                        modifier = Modifier.fillMaxWidth(),
                        primary = false,
                    )
                }
                if (state.redoAnswered && outcome != null) {
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
                    XbButton(
                        "完成，返回错题本",
                        onClick = {
                            state.loadWrong()
                            onClose()
                        },
                        modifier = Modifier.fillMaxWidth().padding(top = 13.dp),
                    )
                }
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
