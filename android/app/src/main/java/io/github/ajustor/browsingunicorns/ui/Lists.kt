package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.Screen

/** Title row with a back arrow, and optional actions on the right. */
@Composable
fun Header(c: BrowserController, title: String, actions: @Composable () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = { c.screen = Screen.Browser }) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back to the page")
        }
        Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        actions()
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
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp),
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

@Composable
fun TabsScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize()) {
        Header(c, "Tabs (${c.state.tabs.size})") {
            IconButton(onClick = {
                c.act { openTab(null) }
                c.screen = Screen.Browser
            }) { Icon(Icons.Filled.Add, contentDescription = "New tab") }
        }
        LazyColumn {
            items(c.state.tabs, key = { it.id.toLong() }) { tab ->
                val active = tab.id == c.state.active
                PageRow(
                    title = (if (active) "● " else "") + tab.title,
                    url = tab.url ?: "",
                    onClick = {
                        c.act { activate(tab.id) }
                        c.screen = Screen.Browser
                    },
                ) {
                    IconButton(onClick = { c.act { closeTab(tab.id) } }) {
                        Icon(Icons.Filled.Close, contentDescription = "Close tab")
                    }
                }
                HorizontalDivider()
            }
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
        Header(c, "History") {
            TextButton(onClick = {
                c.act { clearHistory() }
                version++
            }, enabled = visits.isNotEmpty()) { Text("Clear") }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            singleLine = true,
            placeholder = { Text("Search history") },
        )
        if (visits.isEmpty()) Text("No pages.", modifier = Modifier.padding(16.dp))
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
        Header(c, "Bookmarks")
        if (bookmarks.isEmpty()) {
            Text("No bookmarks. The star at the bottom saves the page.", modifier = Modifier.padding(16.dp))
        }
        LazyColumn {
            items(bookmarks, key = { it.url }) { b ->
                PageRow(b.title, b.url, onClick = { c.open(b.url) }) {
                    IconButton(onClick = {
                        c.act { removeBookmark(b.url) }
                        version++
                    }) { Icon(Icons.Filled.Delete, contentDescription = "Remove bookmark") }
                }
                HorizontalDivider()
            }
        }
    }
}

/** "now", "5 min", "3 h", "2 d". */
fun ago(now: Long, at: Long): String {
    val secs = (now - at).coerceAtLeast(0)
    return when {
        secs < 60 -> "now"
        secs < 3_600 -> "${secs / 60} min"
        secs < 86_400 -> "${secs / 3_600} h"
        else -> "${secs / 86_400} d"
    }
}
