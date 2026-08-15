package com.xueban.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

/** 模考页：倒计时 + 答题卡 + 逐题作答，与原型 pageMock 对应。 */
@Composable
fun MockScreen(state: AppState, onFinish: () -> Unit) {
    val paper = state.mockPaper ?: return
    val n = paper.questions.size
    val q = paper.questions.getOrNull(state.mockIdx)
    var confirmSubmit by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            if (state.mockPaper == null) break
            state.mockSecs--
            if (state.mockSecs <= 0) {
                state.submitMock(onDone = onFinish)
                break
            }
        }
    }

    Column(Modifier.fillMaxSize().background(Xb.bg)) {
        // 顶部：退出 / 标题 / 计时
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                "← 退出",
                modifier = Modifier
                    .clip(RoundedCornerShape(999.dp))
                    .background(Xb.surface2)
                    .clickable {
                        state.mockPaper = null
                        state.toast("已退出模考（进度不保留 · 演示）")
                    }
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                color = Xb.muted, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
            )
            Text(
                paper.paper.name,
                color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.Bold,
                modifier = Modifier.weight(1f),
                maxLines = 1,
            )
            Text(
                "${(state.mockSecs / 60).toString().padStart(2, '0')}:${(state.mockSecs % 60).toString().padStart(2, '0')}",
                modifier = Modifier
                    .clip(RoundedCornerShape(999.dp))
                    .background(Xb.redLight)
                    .padding(horizontal = 12.dp, vertical = 5.dp),
                color = Xb.red, fontSize = 13.sp, fontWeight = FontWeight.Bold,
            )
        }

        // 答题卡
        MockDots(state, n)

        // 题目卡
        if (q == null) {
            Text(
                "试卷为空",
                color = Xb.mutedLight, fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(vertical = 40.dp),
                textAlign = TextAlign.Center,
            )
        } else {
            Column(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp)
            ) {
                MockCard(state, q, state.mockIdx, n)
                Spacer(Modifier.height(14.dp))
            }
        }

        // 底部导航
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            XbButton("上一题", onClick = {
                state.mockIdx = (state.mockIdx - 1).coerceAtLeast(0)
            }, modifier = Modifier.weight(1f).padding(vertical = 0.dp), primary = false, textColor = Xb.muted)
            XbButton("下一题", onClick = {
                state.mockIdx = (state.mockIdx + 1).coerceAtMost(n - 1)
            }, modifier = Modifier.weight(1f).padding(vertical = 0.dp), primary = false, textColor = Xb.muted)
            XbButton("交卷", onClick = {
                val blank = paper.questions.indices.count { !state.mockAnswers.containsKey(it) }
                if (blank > 0) confirmSubmit = true else state.submitMock(onDone = onFinish)
            }, modifier = Modifier.weight(1f).padding(vertical = 0.dp))
        }
    }

    if (confirmSubmit) {
        AlertDialog(
            onDismissRequest = { confirmSubmit = false },
            title = { Text("确认交卷", fontWeight = FontWeight.Bold) },
            text = {
                val blank = paper.questions.indices.count { !state.mockAnswers.containsKey(it) }
                Text("还有 $blank 题未作答，确定交卷吗？")
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmSubmit = false
                    state.submitMock(onDone = onFinish)
                }) { Text("确定交卷", color = Xb.accentDeep) }
            },
            dismissButton = {
                TextButton(onClick = { confirmSubmit = false }) { Text("继续答题", color = Xb.muted) }
            },
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MockDots(state: AppState, n: Int) {
    FlowRow(
        Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        repeat(n) { i ->
            val cur = i == state.mockIdx
            val answered = state.mockAnswers.containsKey(i)
            Box(
                Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(9.dp))
                    .background(
                        when {
                            cur -> Xb.accent
                            answered -> Xb.accentLight
                            else -> Xb.surface
                        }
                    )
                    .border(
                        1.dp,
                        when {
                            cur -> Xb.accent
                            answered -> Xb.accent
                            else -> Xb.border
                        },
                        RoundedCornerShape(9.dp),
                    )
                    .clickable { state.mockIdx = i },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "${i + 1}",
                    color = when {
                        cur -> Color.White
                        answered -> Xb.accentDeep
                        else -> Xb.muted
                    },
                    fontSize = 12.sp, fontWeight = FontWeight.Bold,
                )
            }
        }
    }
}

@Composable
private fun MockCard(state: AppState, q: QuestionBrief, i: Int, n: Int) {
    XbCard {
        Column(Modifier.padding(16.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                val (bg, fg, label) = badgeAi2()
                Badge("模考 · 第 ${i + 1}/$n 题", bg, fg)
                Badge(state.epNameOf(q.sourceItemId), Xb.surface2, Xb.muted)
            }
            Text(
                q.stem,
                color = Xb.ink, fontSize = 14.5.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp,
                modifier = Modifier.padding(top = 11.dp, bottom = 13.dp),
            )
            q.options.forEachIndexed { idx, opt ->
                QOption(
                    key = ('A' + idx).toChar(),
                    text = opt,
                    accent = if (state.mockPicked(idx)) Xb.accent else null,
                    modifier = Modifier.padding(bottom = 9.dp),
                    onClick = { state.mockPick(idx) },
                )
            }
        }
    }
}

/** 模考结果弹层。 */
@Composable
fun ResultSheet(state: AppState, onGoWrong: () -> Unit) {
    val result = state.mockResult ?: return
    val correct = result.correct
    val total = result.total.coerceAtLeast(1)

    XbSheet(
        open = true,
        onDismiss = { state.resultSheet = false },
        title = "模考结果",
        subtitle = null,
    ) {
        Column(Modifier.padding(horizontal = 18.dp)) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(
                    "${result.score}",
                    color = Xb.accentDeep, fontSize = 46.sp, fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Serif,
                    modifier = Modifier.weight(1f),
                    textAlign = TextAlign.Center,
                )
                Text(
                    "/ ${total * 2} 分",
                    color = Xb.mutedLight, fontSize = 15.sp,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
            }
            Text(
                "正确率 ${correct * 100 / total}% · 用时 ${result.durationSecs / 60} 分钟（演示数据）",
                color = Xb.muted, fontSize = 12.sp,
                modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
                textAlign = TextAlign.Center,
            )
            ResultRow2("答对", "$correct 题", Xb.green, first = true)
            ResultRow2("答错", "${total - correct} 题", Xb.red, first = false)
            ResultRow2("错题处理", "已自动归集到错题本", Xb.muted, first = false)
            XbButton("去错题本查看", onClick = {
                state.resultSheet = false
                onGoWrong()
            }, modifier = Modifier.fillMaxWidth().padding(top = 16.dp))
            Spacer(Modifier.height(10.dp))
        }
    }
}

@Composable
private fun ResultRow2(label: String, value: String, valueColor: Color, first: Boolean) {
    Column(Modifier.fillMaxWidth()) {
        if (!first) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(Xb.borderLight))
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 2.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(label, color = Xb.muted, fontSize = 13.sp)
            Text(value, color = valueColor, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}
