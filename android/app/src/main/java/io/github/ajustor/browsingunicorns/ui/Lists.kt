package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.R
import io.github.ajustor.browsingunicorns.Screen

/** Title row with a back arrow, and optional actions on the right. */
@Composable
fun Header(c: BrowserController, title: String, actions: @Composable () -> Unit = {}) {
    CompositionLocalProvider(LocalContentColor provides Color.White) {
        Row(
            Modifier.fillMaxWidth().background(UnicornGradient).statusBarsPadding().padding(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = { c.screen = Screen.Browser }) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back_to_the_page))
            }
            Text(title, style = MaterialTheme.typography.titleLarge, color = Color.White, modifier = Modifier.weight(1f))
            actions()
        }
    }
}

/** A page: title on top, address below. */
@Composable
fun PageRow(
    title: String,
    url: String,
    detail: String? = null,
    onClick: () -> Unit,
    trailing: @Composable () -> Unit = {},
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 3.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    title.ifBlank { url },
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    listOfNotNull(url, detail).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            trailing()
        }
    }
}

@Composable
fun HistoryScreen(c: BrowserController) {
    var query by remember { mutableStateOf("") }
    var version by remember { mutableStateOf(0) }
    val visits = remember(query, version) { c.browser.history(query, 300u) }
    val now = System.currentTimeMillis() / 1000
    Column(Modifier.fillMaxSize()) {
        Header(c, stringResource(R.string.history)) {
            TextButton(onClick = {
                c.act { clearHistory() }
                version++
            }, enabled = visits.isNotEmpty(), colors = ButtonDefaults.textButtonColors(contentColor = Color.White)) {
                Text(stringResource(R.string.clear))
            }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            singleLine = true,
            placeholder = { Text(stringResource(R.string.search_history)) },
        )
        if (visits.isEmpty()) Text(stringResource(R.string.no_pages), modifier = Modifier.padding(16.dp))
        LazyColumn {
            items(visits) { v ->
                PageRow(v.title, v.url, ago(now, v.at.toLong()), onClick = { c.open(v.url) })
            }
        }
    }
}

@Composable
fun BookmarksScreen(c: BrowserController) {
    var version by remember { mutableStateOf(0) }
    val bookmarks = remember(version) { c.browser.bookmarks() }
    Column(Modifier.fillMaxSize()) {
        Header(c, stringResource(R.string.bookmarks))
        if (bookmarks.isEmpty()) {
            Text(stringResource(R.string.no_bookmarks), modifier = Modifier.padding(16.dp))
        }
        LazyColumn {
            items(bookmarks, key = { it.url }) { b ->
                PageRow(b.title, b.url, onClick = { c.open(b.url) }) {
                    IconButton(onClick = {
                        c.act { removeBookmark(b.url) }
                        version++
                    }) { Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.remove_bookmark)) }
                }
            }
        }
    }
}

/** How long ago, short: "just now", "5 min ago", "3 h ago", "2 d ago". */
@Composable
fun ago(now: Long, at: Long): String {
    val secs = (now - at).coerceAtLeast(0)
    return when {
        secs < 60 -> stringResource(R.string.ago_now)
        secs < 3_600 -> stringResource(R.string.ago_minutes, secs / 60)
        secs < 86_400 -> stringResource(R.string.ago_hours, secs / 3_600)
        else -> stringResource(R.string.ago_days, secs / 86_400)
    }
}
