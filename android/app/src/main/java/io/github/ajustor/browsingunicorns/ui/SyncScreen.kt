package io.github.ajustor.browsingunicorns.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.R

@Composable
fun SyncScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize()) {
        Header(c, stringResource(R.string.sync))
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (c.sync.enabled) SyncOn(c) else SyncOff(c)
        }
    }
}

@Composable
private fun SyncOff(c: BrowserController) {
    val context = LocalContext.current
    var code by remember { mutableStateOf("") }
    val scannerUnavailable = stringResource(R.string.sync_scanner_unavailable)
    Text(stringResource(R.string.sync_intro))
    Button(onClick = { c.syncTurnOn() }) { Text(stringResource(R.string.sync_turn_on)) }
    HorizontalDivider()
    Text(stringResource(R.string.sync_join_intro))
    OutlinedButton(onClick = {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { barcode -> barcode.rawValue?.let(c::syncJoin) }
            .addOnFailureListener { c.message = scannerUnavailable }
    }) { Text(stringResource(R.string.sync_scan)) }
    OutlinedTextField(
        value = code,
        onValueChange = { code = it },
        modifier = Modifier.fillMaxWidth(),
        placeholder = { Text("ABCD-EFGH-…") },
        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
    )
    Button(onClick = { c.syncJoin(code) }, enabled = code.isNotBlank()) { Text(stringResource(R.string.sync_join)) }
}

@Composable
private fun SyncOn(c: BrowserController) {
    val info = c.sync
    val now = System.currentTimeMillis() / 1000
    var showCode by remember { mutableStateOf(false) }
    val context = LocalContext.current

    Text(stringResource(R.string.sync_this_device, info.deviceName ?: ""))
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        when {
            c.syncing -> {
                CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                Text(stringResource(R.string.sync_syncing))
            }
            info.lastError != null -> Text(info.lastError!!, color = MaterialTheme.colorScheme.error)
            info.lastSyncAt != null -> Text(stringResource(R.string.sync_synced, ago(now, info.lastSyncAt!!.toLong())))
            else -> Text(stringResource(R.string.sync_never))
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = { c.syncNow() }, enabled = !c.syncing) { Text(stringResource(R.string.sync_now)) }
        OutlinedButton(onClick = { c.syncTurnOff() }) { Text(stringResource(R.string.sync_turn_off)) }
    }

    HorizontalDivider()
    Text(stringResource(R.string.sync_add_device), style = MaterialTheme.typography.titleMedium)
    Text(stringResource(R.string.sync_add_device_intro))
    if (!showCode) {
        OutlinedButton(onClick = { showCode = true }) { Text(stringResource(R.string.sync_show_code)) }
    } else {
        info.qrText?.let { QrCode(it) }
        SelectionContainer {
            Text(info.code ?: "", fontFamily = FontFamily.Monospace)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(onClick = { copy(context, info.code ?: "") }) { Text(stringResource(R.string.sync_copy)) }
            TextButton(onClick = { showCode = false }) { Text(stringResource(R.string.sync_hide)) }
        }
    }

    HorizontalDivider()
    Text(stringResource(R.string.sync_other_devices), style = MaterialTheme.typography.titleMedium)
    if (info.devices.isEmpty()) Text(stringResource(R.string.sync_no_devices))
    for (device in info.devices) {
        Text(
            pluralStringResource(
                R.plurals.sync_device,
                device.tabs.size,
                device.name,
                device.tabs.size,
                ago(now, device.updatedAt.toLong()),
            ),
            style = MaterialTheme.typography.titleSmall,
        )
        for (tab in device.tabs) {
            PageRow(tab.title, tab.url, onClick = { c.open(tab.url, newTab = true) })
        }
        Spacer(Modifier.height(8.dp))
    }
}

/** `text` as a QR code, black on white whatever the theme (scanners expect it). */
@Composable
private fun QrCode(text: String) {
    val bitmap = remember(text) {
        val size = 512
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size)
        Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888).apply {
            for (x in 0 until size) for (y in 0 until size) {
                setPixel(x, y, if (matrix[x, y]) android.graphics.Color.BLACK else android.graphics.Color.WHITE)
            }
        }
    }
    Image(bitmap.asImageBitmap(), contentDescription = stringResource(R.string.sync_qr_code), modifier = Modifier.size(240.dp))
}

private fun copy(context: Context, text: String) {
    context.getSystemService(ClipboardManager::class.java)
        ?.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.sync_code_clip_label), text))
}
