package com.xueban.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** 我的页：账号卡片 + 倒计时 + 菜单 + Agent 接入，与原型 pageMe 对应。 */
@Composable
fun MeScreen(state: AppState, onOpenGoal: () -> Unit) {
    val name = state.user?.nickname ?: state.user?.account ?: "同学"

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
        AppBar(title = "我的")
        Column(Modifier.padding(horizontal = 16.dp)) {
            // me-card
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(Xb.surface)
                    .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
                    .padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Avatar(name, size = 48, fontSize = 19)
                Column(Modifier.weight(1f)) {
                    Text(name, color = Xb.ink, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text(
                        state.examGoal.ifBlank { "未设置考试目标" },
                        color = Xb.muted, fontSize = 12.sp,
                        modifier = Modifier.padding(top = 3.dp),
                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
                Badge("备考中", Xb.accentLight, Xb.accentDeep)
            }

            // countdown-card
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(top = 12.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(Xb.surface)
                    .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
                    .clickable(onClick = onOpenGoal)
                    .padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.Bottom) {
                    Text(
                        "${state.daysLeft()}",
                        color = Xb.accentDeep, fontSize = 34.sp, fontWeight = FontWeight.Bold,
                        fontFamily = FontFamily.Serif,
                    )
                    Text("天", color = Xb.ink, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(bottom = 4.dp, start = 3.dp))
                }
                Spacer(Modifier.size(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("距离考试", color = Xb.mutedLight, fontSize = 11.5.sp)
                    Text(
                        state.examGoal.ifBlank { "未设置考试目标" },
                        color = Xb.ink, fontSize = 13.5.sp, fontWeight = FontWeight.Bold,
                        modifier = Modifier.padding(top = 2.dp),
                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                    Text(state.examDate.ifBlank { "未设置日期" }, color = Xb.muted, fontSize = 11.5.sp, modifier = Modifier.padding(top = 2.dp))
                }
            }

            // me-menu #1
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(top = 12.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(Xb.surface)
                    .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
            ) {
                MeItem("🔌", "Agent 接入凭证") { state.agentSheetOpen = true }
                DividerLine()
                MeItem("🎯", "考试目标设置") { onOpenGoal() }
                DividerLine()
                MeItem("📊", "学习统计") {
                    state.toast("学习统计：已学 1,240 分钟 · 刷题 328 题（演示）")
                }
                DividerLine()
                MeItem("💳", "订阅与计费") {
                    state.toast("订阅计费将在正式版上线（演示）")
                }
            }

            // me-menu #2
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(top = 12.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(Xb.surface)
                    .border(1.dp, Xb.borderLight, RoundedCornerShape(14.dp))
            ) {
                MeItem("👋", "退出登录", danger = true) { state.confirmLogout = true }
            }

            Text(
                "学伴 · 超级学习助手 v0.1（安卓端原型）\n数据按用户隔离 · Agent 仅读写你本人的数据",
                color = Xb.mutedLight, fontSize = 11.sp, lineHeight = 19.sp,
                modifier = Modifier.fillMaxWidth().padding(top = 20.dp, bottom = 24.dp),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
    }
}

@Composable
private fun MeItem(icon: String, label: String, danger: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(icon, fontSize = 17.sp)
        Spacer(Modifier.size(10.dp))
        Text(
            label,
            color = if (danger) Xb.red else Xb.ink,
            fontSize = 13.5.sp, fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f),
        )
        Text("›", color = Xb.mutedLight, fontSize = 12.sp)
    }
}

@Composable
private fun DividerLine() {
    Box(Modifier.fillMaxWidth().padding(start = 40.dp).height(1.dp).background(Xb.borderLight))
}

/** Agent 接入凭证弹层。 */
@Composable
fun AgentSheet(state: AppState, onDismiss: () -> Unit) {
    val clipboard = LocalClipboardManager.current

    LaunchedEffect(Unit) {
        if (state.credential == null) state.loadCredential()
    }

    XbSheet(
        open = true,
        onDismiss = onDismiss,
        title = "🔌 接入 AI Agent",
        subtitle = "复制凭证发给任意 Agent（如 TRAE）",
        maxHeightFraction = 0.9f,
    ) {
        Column(Modifier.padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
            AgentStep(1, "复制接入凭证", "点下方按钮复制")
            AgentStep(2, "发送给任意 Agent", "粘贴到 Agent 对话中")
            AgentStep(3, "自动装配能力", "服务返回 Skill、提示词、MCP 配置")
            AgentStep(4, "开始协作", "Agent 生成目录/笔记/批注/习题，读取错题做复盘")
            Spacer(Modifier.height(6.dp))
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(12.dp))
                    .background(Xb.configBg)
                    .padding(horizontal = 13.dp, vertical = 15.dp)
            ) {
                Text(
                    state.agentConfigText,
                    color = Xb.configFg, fontSize = 11.5.sp, lineHeight = 20.sp,
                    fontFamily = FontFamily.Monospace,
                )
            }
            Spacer(Modifier.height(10.dp))
            Text(
                "🔒 凭证与当前登录用户绑定，Agent 只能读写你本人的学习数据，用户之间严格隔离。",
                color = Xb.gold, fontSize = 11.5.sp, lineHeight = 17.sp,
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(9.dp))
                    .background(Xb.goldLight)
                    .padding(horizontal = 12.dp, vertical = 9.dp),
            )
            Spacer(Modifier.height(14.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                XbButton("稍后接入", onClick = {
                    onDismiss()
                    state.toast("随时可在「我的 → Agent 接入凭证」重新打开")
                }, modifier = Modifier.weight(1f), primary = false)
                XbButton("📋 复制接入凭证", onClick = {
                    clipboard.setText(AnnotatedString(state.agentConfigText))
                    state.toast("接入凭证已复制，发送给任意 Agent 即可接入")
                }, modifier = Modifier.weight(1.4f))
            }
            Spacer(Modifier.height(10.dp))
        }
    }
}

@Composable
private fun AgentStep(no: Int, title: String, desc: String) {
    Row(Modifier.fillMaxWidth().padding(bottom = 9.dp), verticalAlignment = Alignment.Top) {
        Box(
            Modifier
                .size(19.dp)
                .clip(RoundedCornerShape(6.dp))
                .background(Xb.accentLight),
            contentAlignment = Alignment.Center,
        ) {
            Text("$no", color = Xb.accentDeep, fontSize = 11.sp, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.size(8.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = Xb.ink, fontSize = 13.5.sp, fontWeight = FontWeight.Bold)
            Text(desc, color = Xb.muted, fontSize = 12.5.sp, lineHeight = 20.sp, modifier = Modifier.padding(top = 1.dp))
        }
    }
}
