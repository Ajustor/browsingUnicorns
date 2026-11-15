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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import io.github.ajustor.browsingunicorns.BrowserController

@Composable
fun SyncScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize()) {
        Header(c, "Sync")
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
    Text(
        "Sync bookmarks, history, open tabs and settings with your other devices. " +
            "Everything is encrypted on the device: the relay cannot read it.",
    )
    Button(onClick = { c.syncTurnOn() }) { Text("Turn on sync") }
    HorizontalDivider()
    Text("Already on another device? Scan its QR code (Sync › Add a device) or type its code.")
    OutlinedButton(onClick = {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { barcode -> barcode.rawValue?.let(c::syncJoin) }
            .addOnFailureListener { c.message = "Scanner unavailable: type the code instead." }
    }) { Text("Scan QR code") }
    OutlinedTextField(
        value = code,
        onValueChange = { code = it },
        modifier = Modifier.fillMaxWidth(),
        placeholder = { Text("ABCD-EFGH-…") },
        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
    )
    Button(onClick = { c.syncJoin(code) }, enabled = code.isNotBlank()) { Text("Join") }
}

@Composable
private fun SyncOn(c: BrowserController) {
    val info = c.sync
    val now = System.currentTimeMillis() / 1000
    var showCode by remember { mutableStateOf(false) }
    val context = LocalContext.current

    Text("This device: ${info.deviceName ?: ""}")
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        when {
            c.syncing -> {
                CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                Text("Syncing…")
            }
            info.lastError != null -> Text(info.lastError!!, color = MaterialTheme.colorScheme.error)
            info.lastSyncAt != null -> Text("Synced ${ago(now, info.lastSyncAt!!.toLong())}.")
            else -> Text("Not synced yet.")
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = { c.syncNow() }, enabled = !c.syncing) { Text("Sync now") }
        OutlinedButton(onClick = { c.syncTurnOff() }) { Text("Turn off") }
    }

    HorizontalDivider()
    Text("Add a device", style = MaterialTheme.typography.titleMedium)
    Text(
        "On the other device, choose \"Join\" and scan this QR code or type the code. " +
            "Keep it secret: it gives access to your data.",
    )
    if (!showCode) {
        OutlinedButton(onClick = { showCode = true }) { Text("Show the code") }
    } else {
        info.qrText?.let { QrCode(it) }
        SelectionContainer {
            Text(info.code ?: "", fontFamily = FontFamily.Monospace)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(onClick = { copy(context, info.code ?: "") }) { Text("Copy") }
            TextButton(onClick = { showCode = false }) { Text("Hide") }
        }
    }

    HorizontalDivider()
    Text("Other devices", style = MaterialTheme.typography.titleMedium)
    if (info.devices.isEmpty()) Text("None yet.")
    for (device in info.devices) {
        Text(
            "${device.name} · ${device.tabs.size} tabs · ${ago(now, device.updatedAt.toLong())}",
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
    Image(bitmap.asImageBitmap(), contentDescription = "Sync QR code", modifier = Modifier.size(240.dp))
}

private fun copy(context: Context, text: String) {
    context.getSystemService(ClipboardManager::class.java)
        ?.setPrimaryClip(ClipData.newPlainText("browsingUnicorns sync code", text))
}
