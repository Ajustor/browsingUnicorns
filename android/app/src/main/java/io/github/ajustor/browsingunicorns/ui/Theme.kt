package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/** The icon's night purple and cyan, as on the desktop. */
object Unicorn {
    val GradientStart = Color(0xFF3A246E)
    val GradientMiddle = Color(0xFF5B3FA6)
    val GradientEnd = Color(0xFF2D8FB3)
    /** Bottom bar. */
    val Night = Color(0xFF241A3D)
    /** Address bar and its outline. */
    val NightDeep = Color(0xFF171126)
    val Outline = Color(0xFF3F3270)
    /** Text and icons on the bars. */
    val OnNight = Color(0xFFE9E4FF)
    val Cyan = Color(0xFF5ADCFF)

    val DarkBackground = Color(0xFF1C1430)
    val DarkSurface = Color(0xFF241A3D)
    val DarkHover = Color(0xFF2A1F47)
    val DarkText = Color(0xFFECE7FF)

    val LightBackground = Color(0xFFFAF8FF)
    val LightSurface = Color(0xFFEFEAFF)
    val LightAccent = Color(0xFF5B3FA6)
    val LightText = Color(0xFF241B3D)
}

/** The top bar's and headers' gradient (diagonal, left to right). */
val UnicornGradient = Brush.linearGradient(
    listOf(Unicorn.GradientStart, Unicorn.GradientMiddle, Unicorn.GradientEnd),
)

/** The loading bar: violet to cyan. */
val ProgressGradient = Brush.horizontalGradient(listOf(Color(0xFF8A5CFF), Unicorn.Cyan))

private val Dark = darkColorScheme(
    primary = Unicorn.Cyan,
    onPrimary = Unicorn.Night,
    secondary = Unicorn.GradientMiddle,
    onSecondary = Color.White,
    background = Unicorn.DarkBackground,
    onBackground = Unicorn.DarkText,
    surface = Unicorn.DarkBackground,
    onSurface = Unicorn.DarkText,
    surfaceVariant = Unicorn.DarkSurface,
    onSurfaceVariant = Color(0xFFB9B0D8),
    surfaceContainer = Unicorn.DarkSurface,
    surfaceContainerHigh = Unicorn.DarkHover,
    outline = Unicorn.Outline,
)

private val Light = lightColorScheme(
    primary = Unicorn.LightAccent,
    onPrimary = Color.White,
    secondary = Unicorn.GradientEnd,
    onSecondary = Color.White,
    background = Unicorn.LightBackground,
    onBackground = Unicorn.LightText,
    surface = Unicorn.LightBackground,
    onSurface = Unicorn.LightText,
    surfaceVariant = Unicorn.LightSurface,
    onSurfaceVariant = Color(0xFF5A4E80),
    surfaceContainer = Unicorn.LightSurface,
    surfaceContainerHigh = Color(0xFFE6E0F5),
    outline = Color(0xFFE0D9F3),
)

@Composable
fun BrowsingUnicornsTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}

/** WCAG contrast ratio of two opaque colors (1 to 21). */
fun contrast(a: Color, b: Color): Double {
    fun channel(v: Float): Double =
        if (v <= 0.04045f) v / 12.92 else Math.pow((v + 0.055) / 1.055, 2.4)
    fun luminance(c: Color) = 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue)
    val (la, lb) = luminance(a) to luminance(b)
    return (maxOf(la, lb) + 0.05) / (minOf(la, lb) + 0.05)
}
