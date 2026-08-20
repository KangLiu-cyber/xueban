package com.xueban.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            XueBanTheme {
                XueBanApp()
            }
        }
    }
}

private data class NavTab(val emoji: String, val label: String)

private val tabs = listOf(
    NavTab("📖", "笔记"),
    NavTab("✍️", "刷题"),
    NavTab("🩹", "错题本"),
    NavTab("📜", "组卷"),
    NavTab("👤", "我的"),
)

@Composable
fun XueBanApp() {
    val state = rememberAppState()

    // 无感登录：启动时若有持久化 token，在 IO 线程校验并恢复会话。
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { state.restoreSession() }
    }

    // 登录后首次进入：自动弹出 Agent 接入
    LaunchedEffect(state.freshEnter) {
        if (state.freshEnter) {
            delay(700)
            state.agentSheetOpen = true
            state.freshEnter = false
        }
    }
    // Toast 自动消失
    LaunchedEffect(state.toastMsg) {
        val msg = state.toastMsg ?: return@LaunchedEffect
        delay(2200)
        if (state.toastMsg == msg) state.toastMsg = null
    }

    // edge-to-edge 下正文整体下移避开状态栏与挖孔；背景先画铺满全屏，再对内容应用 inset padding。
    Box(Modifier.fillMaxSize().background(Xb.bg).statusBarsPadding()) {
        when {
            // 启动无感恢复中：显示加载，避免登录页闪现。
            state.restoring -> RestoringView()
            !state.loggedIn -> LoginScreen(state, onEntered = {})
            else -> Column(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    TabContent(state)
                }
                NavigationBar(containerColor = Xb.surface) {
                    tabs.forEachIndexed { i, tab ->
                        NavigationBarItem(
                            selected = state.tab == i,
                            onClick = { state.tab = i },
                            icon = { Text(tab.emoji, fontSize = 17.sp) },
                            label = { Text(tab.label, fontSize = 10.5.sp, fontWeight = FontWeight.Medium) },
                        )
                    }
                }
            }
        }

        // 全屏覆盖：模考 / 错题重做
        if (state.mockPaper != null) {
            MockScreen(state, onFinish = {})
        }
        if (state.redoIdx >= 0 && state.mockPaper == null) {
            RedoOverlay(state, onClose = { state.redoIdx = -1 })
        }

        // Sheets
        if (state.goalSheet) GoalSheet(state)
        if (state.annoSheet) AnnoSheet(state)
        if (state.annoDetail != null) AnnoDetailSheet(state)
        if (state.previewSheet && state.mockPaper == null) {
            PreviewSheet(state = state, onStartMock = { state.startMock(state.previewPaper!!) })
        }
        if (state.resultSheet && state.mockPaper == null) {
            ResultSheet(state, onGoWrong = { state.tab = 2 })
        }
        if (state.agentSheetOpen) AgentSheet(state, onDismiss = {
            state.agentSheetOpen = false
            state.toast("随时可在「我的 → Agent 接入凭证」重新打开")
        })
        if (state.skillSheetOpen) SkillSheet(state, onDismiss = {
            state.skillSheetOpen = false
        })

        if (state.confirmLogout) {
            AlertDialog(
                onDismissRequest = { state.confirmLogout = false },
                title = { Text("退出登录", fontWeight = FontWeight.Bold) },
                text = { Text("确定退出当前账号吗？退出后将回到登录页。") },
                confirmButton = {
                    TextButton(onClick = {
                        state.confirmLogout = false
                        state.logout()
                    }) { Text("确定退出", color = Xb.red) }
                },
                dismissButton = {
                    TextButton(onClick = { state.confirmLogout = false }) { Text("取消", color = Xb.muted) }
                },
            )
        }

        ToastHost(state.toastMsg)
    }
}

@Composable
private fun TabContent(state: AppState) {
    when (state.tab) {
        0 -> NotesScreen(state, onTabToMe = { state.tab = 4 })
        1 -> QuizScreen(state, onOpenGoal = { state.goalSheet = true })
        2 -> WrongScreen(state)
        3 -> AssemblyScreen(state, onOpenGoal = { state.goalSheet = true })
        else -> MeScreen(state, onOpenGoal = { state.goalSheet = true })
    }
}

/** 无感登录恢复中的启动加载视图。 */
@Composable
private fun RestoringView() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("正在恢复登录…", color = Xb.muted, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}
