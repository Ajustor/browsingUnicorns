package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.Screen

/** The open tabs as cards: title, favicon and how the page last looked. */
@Composable
fun TabsScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        Header(c, "Tabs (${c.state.tabs.size})") {
            IconButton(onClick = {
                c.act { openTab(null, false) }
                c.screen = Screen.Browser
            }) { Icon(Icons.Filled.Add, contentDescription = "New tab") }
        }
        LazyVerticalGrid(
            columns = GridCells.Fixed(2),
            contentPadding = PaddingValues(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxSize().navigationBarsPadding(),
        ) {
            items(c.state.tabs, key = { it.id.toLong() }) { tab ->
                TabCard(
                    title = tab.title.ifBlank { tab.url ?: "New tab" },
                    host = tab.url?.let { android.net.Uri.parse(it).host } ?: "",
                    active = tab.id == c.state.active,
                    thumbnail = c.views.thumbnails[tab.id],
                    favicon = c.views.favicons[tab.id],
                    onOpen = {
                        c.act { activate(tab.id) }
                        c.screen = Screen.Browser
                    },
                    onClose = { c.act { closeTab(tab.id) } },
                )
            }
        }
    }
}

@Composable
private fun TabCard(
    title: String,
    host: String,
    active: Boolean,
    thumbnail: ImageBitmap?,
    favicon: ImageBitmap?,
    onOpen: () -> Unit,
    onClose: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(14.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = if (active) BorderStroke(2.dp, Unicorn.Cyan) else null,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onOpen),
    ) {
        Column {
            Row(Modifier.padding(start = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                Favicon(favicon, host, 16)
                Text(
                    title,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).padding(start = 6.dp),
                )
                IconButton(onClick = onClose) {
                    Icon(Icons.Filled.Close, contentDescription = "Close tab", modifier = Modifier.size(18.dp))
                }
            }
            Box(
                Modifier.fillMaxWidth().aspectRatio(3f / 4f).background(Unicorn.Night),
                contentAlignment = Alignment.Center,
            ) {
                if (thumbnail != null) {
                    Image(
                        thumbnail,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        alignment = Alignment.TopCenter,
                        modifier = Modifier.fillMaxSize(),
                    )
                } else {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Favicon(favicon, host, 40)
                        Spacer(Modifier.height(8.dp))
                        Text(host, color = Unicorn.OnNight, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                    }
                }
            }
        }
    }
}

/** The page's icon, or its domain's initial in a dot. */
@Composable
private fun Favicon(icon: ImageBitmap?, host: String, size: Int) {
    if (icon != null) {
        Image(icon, contentDescription = null, modifier = Modifier.size(size.dp).clip(RoundedCornerShape(4.dp)))
    } else {
        Box(
            Modifier.size(size.dp).clip(CircleShape).background(UnicornGradient),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                host.removePrefix("www.").take(1).uppercase(),
                color = Color.White,
                style = if (size >= 32) MaterialTheme.typography.titleLarge else MaterialTheme.typography.labelSmall,
            )
        }
    }
}
