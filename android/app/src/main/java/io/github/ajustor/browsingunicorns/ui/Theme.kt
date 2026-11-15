package io.github.ajustor.browsingunicorns.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

// The icon's night purple and its glowing cyan.
private val Purple = Color(0xFF3A246E)
private val Cyan = Color(0xFF5ADCFF)

@Composable
fun BrowsingUnicornsTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val colors = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme(primary = Cyan, secondary = Purple)
        else -> lightColorScheme(primary = Purple, secondary = Cyan)
    }
    MaterialTheme(colorScheme = colors, content = content)
}
