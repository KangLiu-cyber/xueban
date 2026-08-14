package com.xueban.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            XueBanApp()
        }
    }
}

private val pages = listOf("学习空间", "刷题", "错题本", "组卷", "复盘")

@Composable
fun XueBanApp() {
    MaterialTheme {
        var selected by rememberSaveable { mutableIntStateOf(0) }
        BoxWithConstraints(Modifier.fillMaxSize()) {
            if (maxWidth < 600.dp) {
                // 折叠屏窄态：底部导航单栏
                Scaffold(
                    bottomBar = {
                        NavigationBar {
                            pages.forEachIndexed { i, page ->
                                NavigationBarItem(
                                    selected = i == selected,
                                    onClick = { selected = i },
                                    icon = {},
                                    label = { Text(page) }
                                )
                            }
                        }
                    }
                ) { innerPadding ->
                    Text(
                        "待实现：${pages[selected]}",
                        Modifier.fillMaxSize().padding(innerPadding).padding(16.dp)
                    )
                }
            } else {
                // 宽态（≥600dp）：侧边导航双栏
                Row(Modifier.fillMaxSize()) {
                    NavigationRail {
                        pages.forEachIndexed { i, page ->
                            NavigationRailItem(
                                selected = i == selected,
                                onClick = { selected = i },
                                icon = {},
                                label = { Text(page) }
                            )
                        }
                    }
                    Text("待实现：${pages[selected]}", Modifier.padding(16.dp))
                }
            }
        }
    }
}
