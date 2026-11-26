package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
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
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toOffset
import androidx.compose.ui.zIndex
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.R
import io.github.ajustor.browsingunicorns.Screen

/**
 * The open tabs as cards: title, favicon and how the page last looked. A
 * switch shows the normal tabs or the private ones; + opens one of the
 * series shown.
 */
@Composable
fun TabsScreen(c: BrowserController) {
    val private = c.showPrivate && c.views.privateSupported
    val tabs = c.state.tabs.filter { it.private == private }
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        Header(c, stringResource(R.string.tabs_title, tabs.size)) {
            IconButton(onClick = {
                c.act { openTab(null, private) }
                c.screen = Screen.Browser
            }) { Icon(Icons.Filled.Add, contentDescription = stringResource(if (private) R.string.new_private_tab else R.string.new_tab)) }
        }
        if (c.views.privateSupported) {
            SingleChoiceSegmentedButtonRow(
                Modifier.fillMaxWidth().background(if (private) PrivateGradient else UnicornGradient)
                    .padding(horizontal = 12.dp, vertical = 6.dp),
            ) {
                SegmentedButton(
                    selected = !private,
                    onClick = { c.showPrivate = false },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                    colors = seriesColors(),
                ) { Text(stringResource(R.string.tabs_normal)) }
                SegmentedButton(
                    selected = private,
                    onClick = { c.showPrivate = true },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                    colors = seriesColors(),
                    icon = { Icon(MaskIcon, contentDescription = null, modifier = Modifier.size(18.dp)) },
                ) { Text(stringResource(R.string.tabs_private)) }
            }
        }
        if (private && tabs.isEmpty()) {
            Text(
                stringResource(R.string.private_tabs_intro),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(24.dp),
            )
        }
        val grid = rememberLazyGridState()
        val drag = remember { TabDrag() }
        LazyVerticalGrid(
            state = grid,
            columns = GridCells.Fixed(2),
            contentPadding = PaddingValues(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxSize().navigationBarsPadding().pointerInput(Unit) {
                // Long press on a card, then drag: it takes the place of the
                // cards it goes over.
                detectDragGesturesAfterLongPress(
                    onDragStart = { drag.start(grid, it) },
                    onDrag = { change, amount ->
                        change.consume()
                        drag.move(grid, amount) { id, target -> c.moveTab(id, target) }
                    },
                    onDragEnd = { drag.stop() },
                    onDragCancel = { drag.stop() },
                )
            },
        ) {
            items(tabs, key = { it.id.toLong() }) { tab ->
                val dragged = drag.tab == tab.id
                TabCard(
                    modifier = if (dragged) {
                        Modifier.zIndex(1f).graphicsLayer {
                            translationX = drag.offset.x
                            translationY = drag.offset.y
                            scaleX = 1.04f
                            scaleY = 1.04f
                            shadowElevation = 12f
                        }
                    } else {
                        Modifier.animateItem()
                    // Asleep (its page unloaded): fainter.
                    }.alpha(if (tab.asleep && tab.id != c.state.active) 0.6f else 1f),
                    title = tab.title.ifBlank { tab.url ?: stringResource(R.string.new_tab) },
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

/** The card being dragged, and how far it is from its place in the grid. */
private class TabDrag {
    var tab by mutableStateOf<ULong?>(null)
        private set
    var offset by mutableStateOf(Offset.Zero)
        private set

    /** Where the card goes once the grid has moved it: until then, no new move. */
    private var awaited: IntOffset? = null

    fun start(grid: LazyGridState, at: Offset) {
        val item = grid.layoutInfo.visibleItemsInfo.firstOrNull {
            Rect(it.offset.toOffset(), Size(it.size.width.toFloat(), it.size.height.toFloat())).contains(at)
        } ?: return
        tab = (item.key as Long).toULong()
        offset = Offset.Zero
        awaited = null
    }

    fun move(grid: LazyGridState, by: Offset, moveTab: (ULong, ULong) -> Unit) {
        val id = tab ?: return
        offset += by
        val items = grid.layoutInfo.visibleItemsInfo
        val item = items.firstOrNull { it.key == id.toLong() } ?: return
        awaited?.let { if (item.offset != it) return }
        awaited = null
        val center = item.offset.toOffset() + offset +
            Offset(item.size.width / 2f, item.size.height / 2f)
        val target = items.firstOrNull {
            it.key != item.key &&
                Rect(it.offset.toOffset(), Size(it.size.width.toFloat(), it.size.height.toFloat()))
                    .contains(center)
        } ?: return
        moveTab(id, (target.key as Long).toULong())
        // The card now sits where the target was: it stays under the finger.
        offset += (item.offset - target.offset).toOffset()
        awaited = target.offset
    }

    fun stop() {
        tab = null
        offset = Offset.Zero
        awaited = null
    }
}

/** The switch's colors, on the header's gradient. */
@Composable
private fun seriesColors() = SegmentedButtonDefaults.colors(
    activeContainerColor = Color.White.copy(alpha = 0.22f),
    activeContentColor = Color.White,
    activeBorderColor = Color.White.copy(alpha = 0.7f),
    inactiveContainerColor = Color.Transparent,
    inactiveContentColor = Color.White,
    inactiveBorderColor = Color.White.copy(alpha = 0.7f),
)

@Composable
private fun TabCard(
    modifier: Modifier = Modifier,
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
        modifier = modifier.fillMaxWidth().clickable(onClick = onOpen),
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
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.close_tab), modifier = Modifier.size(18.dp))
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
