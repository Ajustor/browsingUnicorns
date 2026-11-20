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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.R

/**
 * Extensions from the Chrome Web Store and user scripts. Android's WebView
 * has no extension engine: the browser applies the extensions' blocking
 * rules and runs their content scripts and the user scripts in pages.
 */
@Composable
fun ExtensionsScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize()) {
        Header(c, stringResource(R.string.extensions))
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                InstallField(
                    c,
                    intro = stringResource(R.string.extensions_intro),
                    label = stringResource(R.string.extension_field),
                    siteLabel = "Chrome Web Store",
                    site = "https://chromewebstore.google.com/",
                    install = c::installExtension,
                )
            }
            if (c.extensions.isEmpty()) item { Empty(stringResource(R.string.no_extensions)) }
            items(c.extensions, key = { "ext:" + it.id }) { e ->
                AddOnRow(e.name, e.version, e.description, e.enabled,
                    onEnabled = { c.setExtensionEnabled(e.id, it) },
                    onRemove = { c.removeExtension(e.id) }) {
                    if (e.hasPopup) TextButton(onClick = { c.openPopup(e.id, e.name) }, enabled = e.enabled) { Text(stringResource(R.string.extension_popup)) }
                    if (e.hasOptions) TextButton(onClick = { c.openOptions(e.id) }, enabled = e.enabled) { Text(stringResource(R.string.extension_options)) }
                }
            }
            item {
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                Text(
                    stringResource(R.string.user_scripts),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
                InstallField(
                    c,
                    intro = stringResource(R.string.user_scripts_intro),
                    label = stringResource(R.string.user_script_field),
                    siteLabel = "Greasy Fork",
                    site = "https://greasyfork.org/",
                    install = c::installUserScript,
                )
            }
            if (c.userScripts.isEmpty()) item { Empty(stringResource(R.string.no_user_scripts)) }
            items(c.userScripts, key = { "us:" + it.id }) { s ->
                AddOnRow(s.name, s.version, s.description, s.enabled,
                    onEnabled = { c.setUserScriptEnabled(s.id, it) },
                    onRemove = { c.removeUserScript(s.id) })
            }
        }
    }
}

@Composable
private fun InstallField(
    c: BrowserController,
    intro: String,
    label: String,
    siteLabel: String,
    site: String,
    install: (String) -> Unit,
) {
    var input by remember { mutableStateOf("") }
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(intro, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(
            value = input,
            onValueChange = { input = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text(label) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = {
                if (input.isNotBlank() && !c.installing) {
                    install(input.trim())
                    input = ""
                }
            }),
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = {
                    install(input.trim())
                    input = ""
                },
                enabled = input.isNotBlank() && !c.installing,
            ) { Text(stringResource(R.string.install)) }
            TextButton(onClick = { c.open(site, newTab = true) }) { Text(siteLabel) }
            if (c.installing) CircularProgressIndicator(Modifier.size(24.dp))
        }
    }
}

@Composable
private fun Empty(text: String) {
    Text(text, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp), style = MaterialTheme.typography.bodyMedium)
}

@Composable
private fun AddOnRow(
    name: String,
    version: String,
    description: String,
    enabled: Boolean,
    onEnabled: (Boolean) -> Unit,
    onRemove: () -> Unit,
    actions: @Composable () -> Unit = {},
) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("$name $version", fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (description.isNotEmpty()) {
                Text(description, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Row { actions() }
        }
        Switch(checked = enabled, onCheckedChange = onEnabled)
        IconButton(onClick = onRemove) {
            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.remove_add_on, name))
        }
    }
}
