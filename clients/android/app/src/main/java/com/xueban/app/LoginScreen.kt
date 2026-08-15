package com.xueban.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** 登录页：两步（账号登录/注册 → 创建备考空间），与原型 pageLogin 对应。 */
@Composable
fun LoginScreen(state: AppState, onEntered: () -> Unit) {
    var step by remember { mutableStateOf(1) }
    var tab by remember { mutableStateOf(0) } // 0 登录 / 1 注册

    var loginAccount by remember { mutableStateOf("") }
    var loginPassword by remember { mutableStateOf("") }
    var regAccount by remember { mutableStateOf("") }
    var regPassword by remember { mutableStateOf("") }
    var regPassword2 by remember { mutableStateOf("") }
    var goal by remember { mutableStateOf("") }
    var date by remember { mutableStateOf("2026-11-07") }

    Box(
        Modifier
            .fillMaxSize()
            .background(Xb.bg)
            .verticalScroll(rememberScrollState())
    ) {
        // 装饰光斑（login-deco）
        Box(
            Modifier
                .size(280.dp)
                .align(Alignment.TopEnd)
                .clip(RoundedCornerShape(140.dp))
                .background(Xb.accentLight.copy(alpha = 0.7f))
        )
        Box(
            Modifier
                .size(240.dp)
                .align(Alignment.BottomStart)
                .clip(RoundedCornerShape(120.dp))
                .background(Xb.goldLight.copy(alpha = 0.8f))
        )

        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 24.dp, vertical = 34.dp)
        ) {
            // 品牌
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Box(
                    Modifier
                        .size(44.dp)
                        .clip(RoundedCornerShape(13.dp))
                        .background(Xb.ink),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("学", color = Xb.bg, fontSize = 22.sp, fontWeight = FontWeight.Bold)
                }
                Column {
                    Text("学伴", fontSize = 19.sp, fontWeight = FontWeight.Bold, color = Xb.ink)
                    Text("超级学习助手 · Android", fontSize = 11.5.sp, color = Xb.muted)
                }
            }
            Spacer(Modifier.height(24.dp))

            if (step == 1) {
                Text(
                    "看课、刷题、备考\n一个空间就够了",
                    fontSize = 26.sp, fontWeight = FontWeight.Bold, lineHeight = 38.sp,
                    color = Xb.ink, fontFamily = FontFamily.Serif,
                )
                Spacer(Modifier.height(10.dp))
                Text(
                    "把课程交给 AI：自动生成笔记与习题；把练习交给系统：刷题、错题、组卷、复盘一气呵成。",
                    fontSize = 13.sp, color = Xb.muted, lineHeight = 23.sp,
                )
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    LoginChip("🤖 AI 生成笔记习题")
                    LoginChip("✏️ 随选随批注")
                }
                Row(horizontalArrangement = Arrangement.spacedBy(7.dp), modifier = Modifier.padding(top = 7.dp)) {
                    LoginChip("🩹 错题自动归集")
                    LoginChip("🔌 任意 Agent 接入")
                }
                Spacer(Modifier.height(22.dp))

                // 登录卡片
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(18.dp))
                        .background(Xb.surface)
                        .padding(20.dp)
                ) {
                    // auth-tabs
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(10.dp))
                            .background(Xb.surface2)
                            .padding(4.dp)
                    ) {
                        AuthTab("登 录", tab == 0) { tab = 0 }
                        AuthTab("注 册", tab == 1) { tab = 1 }
                    }
                    Spacer(Modifier.height(18.dp))

                    if (tab == 0) {
                        FormRow("账号") {
                            FormInput(loginAccount, { loginAccount = it }, placeholder = "请输入账号")
                        }
                        FormRow("密码") {
                            FormInput(loginPassword, { loginPassword = it }, placeholder = "请输入密码", password = true)
                        }
                        XbButton("登 录", modifier = Modifier.fillMaxWidth(), onClick = {
                            when {
                                loginAccount.isBlank() -> state.toast("请输入账号")
                                loginPassword.isBlank() -> state.toast("请输入密码")
                                else -> {
                                    if (state.login(loginAccount, loginPassword)) {
                                        step = 2
                                        goal = ""
                                    }
                                }
                            }
                        })
                    } else {
                        FormRow("账号") {
                            FormInput(regAccount, { regAccount = it }, placeholder = "请输入账号")
                        }
                        FormRow("密码（至少 8 位）") {
                            FormInput(regPassword, { regPassword = it }, placeholder = "请输入密码（至少 8 位）", password = true)
                        }
                        FormRow("确认密码") {
                            FormInput(regPassword2, { regPassword2 = it }, placeholder = "请再次输入密码", password = true)
                        }
                        XbButton("注 册", modifier = Modifier.fillMaxWidth(), onClick = {
                            when {
                                regAccount.isBlank() -> state.toast("请输入账号")
                                regPassword.length < 8 -> state.toast("密码至少 8 位")
                                regPassword != regPassword2 -> state.toast("两次输入的密码不一致")
                                else -> {
                                    if (state.register(regAccount, regPassword)) {
                                        state.toast("注册成功，请登录")
                                        loginAccount = regAccount
                                        loginPassword = ""
                                        tab = 0
                                    }
                                }
                            }
                        })
                    }
                    Text(
                        "账号体系是订阅计费与学习数据存储的基础\n登录即代表同意《用户协议》与《隐私政策》（演示文案）",
                        Modifier.fillMaxWidth().padding(top = 14.dp),
                        color = Xb.mutedLight, fontSize = 11.sp, textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        lineHeight = 19.sp,
                    )
                }
            } else {
                // 第二步：创建备考空间
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                    StepChip("① 账号登录 ✓", done = true)
                    Line()
                    StepChip("② 创建备考空间", done = false)
                }
                Spacer(Modifier.height(18.dp))
                Text("🎯 写下你的考试目标", fontSize = 15.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink)
                Text(
                    "自由填写，写清楚就行，不设下拉选项",
                    Modifier.padding(top = 5.dp),
                    color = Xb.mutedLight, fontSize = 12.sp,
                )
                Spacer(Modifier.height(14.dp))
                FormRow("考试目标（手写填写）") {
                    FormInput(goal, { goal = it }, placeholder = "如：软考 · 系统架构设计师")
                }
                FormRow("考试日期") {
                    FormInput(date, { date = it }, placeholder = "2026-11-07")
                }
                XbButton("确认并进入系统", modifier = Modifier.fillMaxWidth(), onClick = {
                    if (goal.isBlank()) {
                        state.toast("请手写填写你的考试目标")
                        return@XbButton
                    }
                    state.ensureWorkspace(goal, goal, date.ifBlank { null })
                    if (state.loggedIn) {
                        state.goalInput = goal
                        state.dateInput = date
                        onEntered()
                    }
                })
                Spacer(Modifier.height(8.dp))
                Text(
                    "← 返回登录",
                    Modifier.align(Alignment.CenterHorizontally).clickable { step = 1 },
                    color = Xb.accentDeep, fontSize = 13.sp, fontWeight = FontWeight.SemiBold,
                )
            }
        }
    }
}

@Composable
private fun LoginChip(text: String) {
    Text(
        text,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Xb.surface)
            .padding(horizontal = 11.dp, vertical = 5.dp),
        color = Xb.muted, fontSize = 11.5.sp, fontWeight = FontWeight.SemiBold,
    )
}

@Composable
private fun RowScope.AuthTab(text: String, active: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .weight(1f)
            .clip(RoundedCornerShape(8.dp))
            .then(
                // 原型 .auth-tab.active 带 box-shadow：激活页签浮起。
                if (active) Modifier.shadow(2.dp, RoundedCornerShape(8.dp), Xb.shadow, Xb.shadow)
                else Modifier
            )
            .background(if (active) Xb.surface else Color.Transparent)
            .clickable(onClick = onClick)
            .padding(vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = if (active) Xb.ink else Xb.muted, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun StepChip(text: String, done: Boolean) {
    Text(
        text,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (done) Xb.greenLight else Xb.accentLight)
            .padding(horizontal = 10.dp, vertical = 3.dp),
        color = if (done) Xb.green else Xb.accentDeep,
        fontSize = 11.5.sp, fontWeight = FontWeight.SemiBold,
    )
}

@Composable
private fun Line() {
    Box(
        Modifier
            .width(14.dp)
            .height(1.5.dp)
            .background(Xb.border)
    )
}
