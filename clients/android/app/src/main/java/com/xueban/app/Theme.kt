package com.xueban.app

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** 配色与字号全部取自 docs/ui-mockup/android-prototype-v1.html 的 :root 变量。 */
object Xb {
    val bg = Color(0xFFF7F5F0)
    val surface = Color(0xFFFFFFFF)
    val surface2 = Color(0xFFF2EFE8)
    val ink = Color(0xFF2B2822)
    val muted = Color(0xFF6F6A5E)
    val mutedLight = Color(0xFF9B958A)
    val accent = Color(0xFF8B7FD6)
    val accentDeep = Color(0xFF6B5FC0)
    val accentLight = Color(0xFFEFECFA)
    val gold = Color(0xFFB98A2F)
    val goldLight = Color(0xFFF7EDD8)
    val green = Color(0xFF3E9B6E)
    val greenLight = Color(0xFFE4F3EB)
    val red = Color(0xFFD6646A)
    val redLight = Color(0xFFFBE9EA)
    val border = Color(0xFFE5E0D5)
    val borderLight = Color(0xFFEEEAE0)
    val configBg = Color(0xFF2F2C26)
    val configFg = Color(0xFFEDE9DF)
    val shadow = Color(0x0F2B2822)
}

@Composable
fun XueBanTheme(content: @Composable () -> Unit) {
    val scheme = lightColorScheme(
        primary = Xb.accent,
        onPrimary = Color.White,
        primaryContainer = Xb.accentLight,
        onPrimaryContainer = Xb.accentDeep,
        secondary = Xb.green,
        background = Xb.bg,
        onBackground = Xb.ink,
        surface = Xb.surface,
        onSurface = Xb.ink,
        surfaceVariant = Xb.surface2,
        onSurfaceVariant = Xb.muted,
        outline = Xb.border,
        error = Xb.red,
    )
    MaterialTheme(
        colorScheme = scheme,
        typography = Typography(
            bodyLarge = TextStyle(fontSize = 14.5.sp, lineHeight = 20.sp),
            bodyMedium = TextStyle(fontSize = 13.5.sp, lineHeight = 20.sp),
            bodySmall = TextStyle(fontSize = 12.sp, lineHeight = 18.sp),
            titleLarge = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold),
            titleMedium = TextStyle(fontSize = 17.sp, fontWeight = FontWeight.Bold),
            labelLarge = TextStyle(fontSize = 14.sp, fontWeight = FontWeight.SemiBold),
            labelMedium = TextStyle(fontSize = 12.sp, fontWeight = FontWeight.SemiBold),
            labelSmall = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.SemiBold),
        ),
        shapes = MaterialTheme.shapes.copy(
            extraLarge = RoundedCornerShape(18.dp),
            large = RoundedCornerShape(14.dp),
            medium = RoundedCornerShape(12.dp),
            small = RoundedCornerShape(10.dp),
        ),
        content = content,
    )
}

/** 与原型一致的卡片投影。 */
val XbCardShadow = androidx.compose.ui.graphics.Shadow(
    color = Xb.shadow,
    offset = androidx.compose.ui.geometry.Offset(0f, 3f),
    blurRadius = 12f,
)
