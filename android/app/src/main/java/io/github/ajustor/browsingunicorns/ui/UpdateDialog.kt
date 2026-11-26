package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import io.github.ajustor.browsingunicorns.AppUpdates
import io.github.ajustor.browsingunicorns.R
import io.github.ajustor.browsingunicorns.UpdateState
import io.github.ajustor.browsingunicorns.releaseNotes

/**
 * The window offering an update found at start, as on the desktop: what it
 * brings, then Update, Later or Skip; the download's progress; then the
 * system's installer. Closed beside it: later (a download goes on).
 */
@Composable
fun UpdateDialog(u: AppUpdates) {
    val state = u.state
    if (!u.prompt) return
    val release = when (state) {
        is UpdateState.Available -> state.release
        is UpdateState.Downloading -> state.release
        is UpdateState.Ready -> state.release
        else -> return
    }
    Dialog(onDismissRequest = { u.hidePrompt() }) {
        Surface(
            shape = RoundedCornerShape(24.dp),
            tonalElevation = 6.dp,
            shadowElevation = 12.dp,
        ) {
            Column {
                Header(state, u.currentVersion, release.version)
                Column(Modifier.padding(horizontal = 20.dp, vertical = 16.dp)) {
                    when (state) {
                        is UpdateState.Available -> Offer(u, release.version, release.notes)
                        is UpdateState.Downloading -> Progress(u, state.percent)
                        is UpdateState.Ready -> Installing(u)
                    }
                }
            }
        }
    }
}

/** The accent band: two glows, the icon, the title and the versions. */
@Composable
private fun Header(state: UpdateState, current: String, next: String) {
    val title = stringResource(
        when (state) {
            is UpdateState.Downloading -> R.string.update_window_downloading
            is UpdateState.Ready -> R.string.update_window_ready
            else -> R.string.update_window_title
        },
    )
    Box(
        Modifier
            .fillMaxWidth()
            .height(112.dp)
            .clip(RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp))
            .background(Unicorn.GradientMiddle),
    ) {
        Canvas(Modifier.matchParentSize()) {
            drawCircle(
                Unicorn.GradientEnd.copy(alpha = 0.3f),
                radius = 62.dp.toPx(),
                center = Offset(size.width - 70.dp.toPx(), size.height + 10.dp.toPx()),
            )
            drawCircle(
                Unicorn.GradientStart.copy(alpha = 0.3f),
                radius = 70.dp.toPx(),
                center = Offset(150.dp.toPx(), size.height + 40.dp.toPx()),
            )
        }
        Row(
            Modifier.matchParentSize().padding(horizontal = 20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(56.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.16f)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(GiftIcon, contentDescription = null, tint = Color.White, modifier = Modifier.size(30.dp))
            }
            Spacer(Modifier.width(16.dp))
            Column {
                Text(
                    title,
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(current, color = Color.White.copy(alpha = 0.86f))
                    Icon(
                        Icons.AutoMirrored.Filled.ArrowForward,
                        contentDescription = null,
                        tint = Color.White.copy(alpha = 0.86f),
                        modifier = Modifier.padding(horizontal = 6.dp).size(14.dp),
                    )
                    Text(next, color = Color.White.copy(alpha = 0.86f))
                }
            }
        }
    }
}

/** What the version brings, then its buttons. */
@Composable
private fun ColumnScope.Offer(u: AppUpdates, version: String, notes: String) {
    val sections = releaseNotes(notes)
    if (sections.isNotEmpty()) {
        Text(
            stringResource(R.string.update_in_this_version, version),
            fontWeight = FontWeight.SemiBold,
        )
        Spacer(Modifier.height(8.dp))
        Column(Modifier.heightIn(max = 260.dp).verticalScroll(rememberScrollState())) {
            for (section in sections) {
                if (section.title.isNotEmpty()) {
                    Text(
                        section.title,
                        color = Unicorn.Cyan,
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.padding(top = 6.dp, bottom = 2.dp),
                    )
                }
                for (item in section.items) {
                    Row(Modifier.padding(vertical = 2.dp)) {
                        Text("•", color = Unicorn.Cyan, modifier = Modifier.padding(end = 8.dp))
                        Text(item, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        }
        Spacer(Modifier.height(16.dp))
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = { u.install() },
            colors = ButtonDefaults.buttonColors(containerColor = Unicorn.GradientMiddle, contentColor = Color.White),
        ) { Text(stringResource(R.string.update_now)) }
        OutlinedButton(onClick = { u.later() }) { Text(stringResource(R.string.update_later)) }
    }
    TextButton(onClick = { u.skip() }, modifier = Modifier.align(Alignment.End)) {
        Text(stringResource(R.string.update_skip_version), color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** The download's progress; the window can be hidden meanwhile. */
@Composable
private fun Progress(u: AppUpdates, percent: Int) {
    Text(stringResource(R.string.update_downloading_long, percent))
    Spacer(Modifier.height(10.dp))
    LinearProgressIndicator(
        progress = { percent / 100f },
        color = Unicorn.Cyan,
        modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(4.dp)),
    )
    Spacer(Modifier.height(12.dp))
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            stringResource(R.string.update_keep_browsing),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = { u.hidePrompt() }) { Text(stringResource(R.string.update_hide)) }
    }
}

/** Downloaded: the system's installer takes over (Install starts it again). */
@Composable
private fun Installing(u: AppUpdates) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = Unicorn.Cyan)
        Spacer(Modifier.width(8.dp))
        Text(stringResource(R.string.update_ready_install))
    }
    Spacer(Modifier.height(16.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = { u.install() },
            colors = ButtonDefaults.buttonColors(containerColor = Unicorn.GradientMiddle, contentColor = Color.White),
        ) { Text(stringResource(R.string.install)) }
        OutlinedButton(onClick = { u.later() }) { Text(stringResource(R.string.update_later)) }
    }
}

/** A gift: a new version. */
private val GiftIcon: ImageVector = ImageVector.Builder(
    name = "Gift",
    defaultWidth = 24.dp,
    defaultHeight = 24.dp,
    viewportWidth = 24f,
    viewportHeight = 24f,
).addPath(
    pathData = addPathNodes(
        // The lid, the box (split by the ribbon), and the bow's two loops.
        "M3,8 H21 V12 H3 Z M4,13 H11 V21 H4 Z M13,13 H20 V21 H13 Z " +
            "M12,8 C10,4 6,4 6.5,6.5 C7,8 10,8 12,8 Z M12,8 C14,4 18,4 17.5,6.5 C17,8 14,8 12,8 Z",
    ),
    pathFillType = PathFillType.NonZero,
    fill = SolidColor(Color.Black),
).build()
