package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController

/**
 * Extensions from the Chrome Web Store. Android's WebView cannot run them:
 * the browser applies their network rules (ad and tracker blocking).
 */
@Composable
fun ExtensionsScreen(c: BrowserController) {
    var input by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize()) {
        Header(c, "Extensions")
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                "Ad and tracker blockers from the Chrome Web Store (uBlock Origin Lite, AdGuard…): " +
                    "their blocking rules apply to every page. Other features of extensions do not run here.",
                style = MaterialTheme.typography.bodyMedium,
            )
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Chrome Web Store link or extension id") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = {
                    if (input.isNotBlank()) c.installExtension(input.trim())
                }),
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = {
                        c.installExtension(input.trim())
                        input = ""
                    },
                    enabled = input.isNotBlank() && !c.installing,
                ) { Text("Install") }
                TextButton(onClick = { c.open("https://chromewebstore.google.com/", newTab = true) }) {
                    Text("Chrome Web Store")
                }
                if (c.installing) CircularProgressIndicator(Modifier.size(24.dp))
            }
        }
        HorizontalDivider(Modifier.padding(top = 8.dp))
        if (c.extensions.isEmpty()) {
            Text(
                "No extensions.",
                modifier = Modifier.padding(16.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(c.extensions, key = { it.id }) { e ->
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(
                            "${e.name} ${e.version}",
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (e.description.isNotEmpty()) {
                            Text(
                                e.description,
                                style = MaterialTheme.typography.bodySmall,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    Switch(checked = e.enabled, onCheckedChange = { c.setExtensionEnabled(e.id, it) })
                    IconButton(onClick = { c.removeExtension(e.id) }) {
                        Icon(Icons.Filled.Delete, contentDescription = "Remove ${e.name}")
                    }
                }
            }
        }
    }
}
