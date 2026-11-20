package io.github.ajustor.browsingunicorns.ui

import android.text.format.DateUtils
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.R
import uniffi.mobile.BlockerInfo

/** A shield (Material's), the core icons have none. */
private val ShieldIcon: ImageVector = ImageVector.Builder(
    name = "Shield",
    defaultWidth = 24.dp,
    defaultHeight = 24.dp,
    viewportWidth = 24f,
    viewportHeight = 24f,
).addPath(
    pathData = addPathNodes("M12,1L3,5v6c0,5.55 3.84,10.74 9,12 5.16,-1.26 9,-6.45 9,-12V5l-9,-4z"),
    fill = SolidColor(Color.Black),
).build()

/** Whether the ad blocker works on the page. */
private val BlockerInfo.active: Boolean get() = enabled && !siteAllowed

/** The address bar's shield, with what was blocked on the page. */
@Composable
fun ShieldButton(info: BlockerInfo, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        BadgedBox(badge = {
            if (info.active && info.blocked > 0u) {
                Badge(containerColor = Unicorn.Cyan, contentColor = Unicorn.Night) {
                    Text(if (info.blocked > 99u) "99+" else info.blocked.toString())
                }
            }
        }) {
            Icon(
                ShieldIcon,
                contentDescription = stringResource(if (info.active) R.string.shield_on else R.string.shield_off),
                tint = if (info.active) Unicorn.Cyan else Unicorn.OnNight.copy(alpha = 0.5f),
                modifier = Modifier.size(20.dp),
            )
        }
    }
}

/** What the shield shows: what was blocked, the switches, the lists. */
@Composable
fun ShieldDialog(
    host: String,
    info: BlockerInfo,
    onSiteChange: (Boolean) -> Unit,
    onBlockerChange: (Boolean) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.close)) } },
        title = { Text(host) },
        text = {
            Column {
                Text(
                    when {
                        !info.enabled -> stringResource(R.string.blocker_off)
                        info.siteAllowed -> stringResource(R.string.blocker_site_allowed)
                        else -> info.blocked.toInt().let { pluralStringResource(R.plurals.items_blocked, it, it) }
                    },
                )
                Spacer(Modifier.height(12.dp))
                SwitchRow(stringResource(R.string.block_on_this_site), !info.siteAllowed, enabled = info.enabled, onSiteChange)
                SwitchRow(stringResource(R.string.ad_blocker), info.enabled, enabled = true, onBlockerChange)
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(
                        R.string.blocker_lists,
                        info.listsUpdated?.let {
                            stringResource(
                                R.string.blocker_lists_updated,
                                DateUtils.getRelativeTimeSpanString(
                                    it * 1000,
                                    System.currentTimeMillis(),
                                    DateUtils.MINUTE_IN_MILLIS,
                                ),
                            )
                        } ?: stringResource(R.string.blocker_lists_not_downloaded),
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
    )
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange, enabled = enabled)
    }
}
