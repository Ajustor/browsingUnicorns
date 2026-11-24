package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

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

/** A private tab's bars: anthracite, as the desktop's private window. */
object Private {
    val GradientStart = Color(0xFF2B2B33)
    val GradientMiddle = Color(0xFF3A3A46)
    val GradientEnd = Color(0xFF2B2B33)
    /** Bottom bar. */
    val Bar = Color(0xFF1F1F26)
}

/** The top bar's gradient when the tab shown is private. */
val PrivateGradient = Brush.linearGradient(
    listOf(Private.GradientStart, Private.GradientMiddle, Private.GradientEnd),
)

/** A domino mask: private tabs. */
val MaskIcon: ImageVector = ImageVector.Builder(
    name = "Mask",
    defaultWidth = 24.dp,
    defaultHeight = 24.dp,
    viewportWidth = 24f,
    viewportHeight = 24f,
).addPath(
    pathData = addPathNodes(
        "M2.5,7.5 C6,6 9,6.5 12,8 C15,6.5 18,6 21.5,7.5 C22,11.5 20.5,15.5 16.5,15.5 " +
            "C14.5,15.5 13.5,13.5 12,13.5 C10.5,13.5 9.5,15.5 7.5,15.5 C3.5,15.5 2,11.5 2.5,7.5 Z " +
            "M6,10 C6.5,8.8 9,8.8 10,10.2 C9,11.4 6.8,11.4 6,10 Z " +
            "M18,10 C17.5,8.8 15,8.8 14,10.2 C15,11.4 17.2,11.4 18,10 Z",
    ),
    pathFillType = PathFillType.EvenOdd,
    fill = SolidColor(Color.Black),
).build()

/** A clock: a page of the history. */
val HistoryIcon: ImageVector = ImageVector.Builder(
    name = "History",
    defaultWidth = 24.dp,
    defaultHeight = 24.dp,
    viewportWidth = 24f,
    viewportHeight = 24f,
).addPath(
    pathData = addPathNodes(
        "M11.99,2C6.47,2 2,6.48 2,12s4.47,10 9.99,10C17.52,22 22,17.52 22,12S17.52,2 11.99,2z" +
            "M12,20c-4.42,0 -8,-3.58 -8,-8s3.58,-8 8,-8 8,3.58 8,8 -3.58,8 -8,8z" +
            "M12.5,7H11v6l5.25,3.15 0.75,-1.23 -4.5,-2.67z",
    ),
    fill = SolidColor(Color.Black),
).build()

/** A key: saved passwords. */
val KeyIcon: ImageVector = ImageVector.Builder(
    name = "Key",
    defaultWidth = 24.dp,
    defaultHeight = 24.dp,
    viewportWidth = 24f,
    viewportHeight = 24f,
).addPath(
    pathData = addPathNodes(
        "M12.65,10C11.83,7.67 9.61,6 7,6c-3.31,0 -6,2.69 -6,6s2.69,6 6,6c2.61,0 4.83,-1.67 5.65,-4H17v4h4v-4h2v-4H12.65z" +
            "M7,14c-1.1,0 -2,-0.9 -2,-2s0.9,-2 2,-2 2,0.9 2,2 -0.9,2 -2,2z",
    ),
    fill = SolidColor(Color.Black),
).build()

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
