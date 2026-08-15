package com.xueban.app

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// ==================== 原子组件（与原型 CSS 一一对应） ====================

/** .card：白底卡片 + 细边框 + 轻投影。 */
@Composable
fun XbCard(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier,
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.cardColors(containerColor = Xb.surface),
        border = BorderStroke(1.dp, Xb.borderLight),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column(Modifier.fillMaxWidth(), content = content)
    }
}

/** .badge：胶囊小标签。 */
@Composable
fun Badge(
    text: String,
    bg: Color = Xb.goldLight,
    fg: Color = Xb.gold,
    modifier: Modifier = Modifier,
) {
    Text(
        text,
        modifier = modifier
            .clip(RoundedCornerShape(999.dp))
            .background(bg)
            .padding(horizontal = 8.dp, vertical = 2.dp),
        color = fg,
        fontSize = 10.5.sp,
        fontWeight = FontWeight.SemiBold,
    )
}

fun badgeAi() = Triple(Xb.goldLight, Xb.gold, "AI 生成")
fun badgeAi2() = Triple(Xb.accentLight, Xb.accentDeep, "AI 生成")
fun badgeGreen() = Triple(Xb.greenLight, Xb.green, "已掌握")
fun badgeRed() = Triple(Xb.redLight, Xb.red, "错题重做")

/** .btn-primary / .btn-ghost；block 语义由调用处用 fillMaxWidth 表达。 */
@Composable
fun XbButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = true,
    small: Boolean = false,
    textColor: Color? = null,
    enabled: Boolean = true,
) {
    val shape = RoundedCornerShape(if (small) 8.dp else 12.dp)
    val bg = if (primary) Xb.accent else Xb.surface2
    val fg = textColor ?: if (primary) Color.White else Xb.ink
    Box(
        modifier = modifier
            .clip(shape)
            .background(bg.copy(alpha = if (enabled) 1f else 0.5f))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(
                horizontal = if (small) 12.dp else 0.dp,
                vertical = if (small) 6.dp else 13.dp,
            ),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = fg, fontSize = if (small) 11.5.sp else 14.5.sp, fontWeight = FontWeight.SemiBold)
    }
}

/** .appbar：标题 + 副标题（可带右侧槽位）。 */
@Composable
fun AppBar(
    title: String,
    subtitle: String? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 16.dp, top = 10.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 17.sp, fontWeight = FontWeight.Bold, color = Xb.ink)
            if (subtitle != null) {
                Text(subtitle, fontSize = 11.5.sp, color = Xb.mutedLight, modifier = Modifier.padding(top = 1.dp))
            }
        }
        trailing?.invoke()
    }
}

/** .cd-chip：倒计时胶囊。 */
@Composable
fun CountdownChip(days: Long, modifier: Modifier = Modifier, onClick: (() -> Unit)? = null) {
    val m = modifier
        .clip(RoundedCornerShape(999.dp))
        .background(Xb.accentLight)
        .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
        .padding(horizontal = 11.dp, vertical = 5.dp)
    Text("⏳ $days 天", modifier = m, color = Xb.accentDeep, fontSize = 11.5.sp, fontWeight = FontWeight.Bold)
}

/** .avatar：圆形头像（首字）。 */
@Composable
fun Avatar(initial: String, size: Int = 32, fontSize: Int = 13, modifier: Modifier = Modifier, onClick: (() -> Unit)? = null) {
    val ch = initial.take(1).ifEmpty { "学" }
    Box(
        modifier = modifier
            .size(size.dp)
            .clip(CircleShape)
            .background(Xb.accentLight)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier),
        contentAlignment = Alignment.Center,
    ) {
        Text(ch, color = Xb.accentDeep, fontSize = fontSize.sp, fontWeight = FontWeight.Bold)
    }
}

/** .form-input：表单输入框。 */
@Composable
fun FormInput(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String = "",
    modifier: Modifier = Modifier,
    password: Boolean = false,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(Xb.surface)
            .border(1.dp, Xb.border, RoundedCornerShape(10.dp))
            .padding(horizontal = 14.dp, vertical = 12.dp)
    ) {
        if (value.isEmpty()) {
            Text(placeholder, color = Xb.mutedLight, fontSize = 14.5.sp)
        }
        androidx.compose.foundation.text.BasicTextField(
            value = value,
            onValueChange = onValueChange,
            textStyle = TextStyle(fontSize = 14.5.sp, color = Xb.ink),
            visualTransformation = if (password)
                androidx.compose.ui.text.input.PasswordVisualTransformation()
            else androidx.compose.ui.text.input.VisualTransformation.None,
        )
    }
}

/** .form-row：标签 + 输入框。 */
@Composable
fun FormRow(label: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(bottom = 14.dp)) {
        Text(label, color = Xb.muted, fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(bottom = 6.dp))
        content()
    }
}

/** .src-hint：虚线提示条。 */
@Composable
fun SrcHint(text: String, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(Xb.surface2)
            .border(1.dp, Xb.border, RoundedCornerShape(10.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(text, color = Xb.muted, fontSize = 11.5.sp, lineHeight = 17.sp)
    }
}

/** .section-title：分组小标题。 */
@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier.padding(start = 2.dp, end = 2.dp, top = 16.dp, bottom = 9.dp),
        color = Xb.mutedLight,
        fontSize = 12.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.4.sp,
    )
}

/** .q-opt：ABCD 选项行（correct/wrong/accent 三态）。 */
@Composable
fun QOption(
    key: Char,
    text: String,
    modifier: Modifier = Modifier,
    accent: Color? = null,
    dimmed: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    val borderColor = when (accent) {
        Xb.green -> Xb.green
        Xb.red -> Xb.red
        else -> accent ?: Xb.border
    }
    val bg = when (accent) {
        Xb.green -> Xb.greenLight
        Xb.red -> Xb.redLight
        else -> accent?.let { Xb.accentLight } ?: Xb.surface
    }
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(if (dimmed && accent == null) bg.copy(alpha = 0.82f) else bg)
            .border(1.5.dp, borderColor, RoundedCornerShape(12.dp))
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 13.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier
                .size(22.dp)
                .clip(CircleShape)
                .background(if (accent != null) accent else Xb.surface2),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                key.toString(),
                color = if (accent != null) Color.White else Xb.muted,
                fontSize = 11.5.sp,
                fontWeight = FontWeight.Bold,
            )
        }
        Text(
            text,
            color = if (dimmed && accent == null) Xb.mutedLight else Xb.ink,
            fontSize = 13.5.sp,
            lineHeight = 21.sp,
        )
    }
}

// ==================== Sheet（底部弹层，与原型 .sheet 对应） ====================

@Composable
fun XbSheet(
    open: Boolean,
    onDismiss: () -> Unit,
    title: String,
    subtitle: String? = null,
    maxHeightFraction: Float = 0.82f,
    content: @Composable () -> Unit,
) {
    if (!open) return
    Box(
        Modifier
            .fillMaxSize()
            .background(Color(0x6B1C1A16))
            .clickable(interactionSource = androidx.compose.foundation.interaction.MutableInteractionSource(),
                indication = null, onClick = onDismiss)
    ) {
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .clip(RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
                .background(Xb.surface)
                .padding(bottom = 26.dp)
        ) {
            Box(
                Modifier
                    .padding(top = 9.dp)
                    .size(width = 38.dp, height = 4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(Xb.border)
                    .align(Alignment.CenterHorizontally)
            )
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(start = 18.dp, end = 18.dp, top = 12.dp, bottom = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(title, fontSize = 15.5.sp, fontWeight = FontWeight.Bold, color = Xb.ink)
                    if (subtitle != null) {
                        Text(subtitle, fontSize = 11.5.sp, color = Xb.mutedLight, modifier = Modifier.padding(top = 1.dp))
                    }
                }
                Box(
                    Modifier
                        .size(30.dp)
                        .clip(CircleShape)
                        .background(Xb.surface2)
                        .clickable(onClick = onDismiss),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("✕", color = Xb.muted, fontSize = 13.sp)
                }
            }
            content()
        }
    }
}

// ==================== Toast ====================

@Composable
fun ToastHost(msg: String?) {
    if (msg == null) return
    Box(Modifier.fillMaxSize()) {
        Box(
            Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = 92.dp)
                .clip(RoundedCornerShape(999.dp))
                .background(Xb.ink)
                .padding(horizontal = 18.dp, vertical = 10.dp)
        ) {
            Text(msg, color = Xb.bg, fontSize = 12.5.sp, fontWeight = FontWeight.Medium)
        }
    }
}
