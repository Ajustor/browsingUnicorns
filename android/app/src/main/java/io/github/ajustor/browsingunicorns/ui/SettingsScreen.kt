package io.github.ajustor.browsingunicorns.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.R

/** Home page, search engine, passwords, and clearing what was browsed. */
@Composable
fun SettingsScreen(c: BrowserController) {
    var version by remember { mutableIntStateOf(0) }
    val settings = remember(version) { c.browser.generalSettings() }
    var home by remember(version) { mutableStateOf(settings.homePage) }
    var history by remember { mutableStateOf(false) }
    var cookies by remember { mutableStateOf(true) }
    var cache by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) { c.refreshCredentials() }
    Column(Modifier.fillMaxSize()) {
        Header(c, stringResource(R.string.settings))
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
            Section(stringResource(R.string.home_page))
            OutlinedTextField(
                value = home,
                onValueChange = { home = it },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = {
                    c.act { setHomePage(home) }
                    version++
                }),
            )
            if (home != settings.homePage) {
                TextButton(onClick = {
                    c.act { setHomePage(home) }
                    version++
                }) { Text(stringResource(R.string.save)) }
            }

            Section(stringResource(R.string.search_engine))
            for (engine in settings.engines) {
                Row(
                    Modifier.fillMaxWidth().clickable {
                        c.act { setSearchEngine(engine) }
                        version++
                    },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = engine == settings.searchEngine, onClick = null)
                    Text(engine, Modifier.padding(start = 8.dp, top = 10.dp, bottom = 10.dp))
                }
            }

            Section(stringResource(R.string.passwords))
            Check(stringResource(R.string.save_passwords), settings.savePasswords) {
                c.act { setSavePasswords(it) }
                version++
            }
            Check(stringResource(R.string.sync_cookies), settings.syncCookies) {
                c.act { setSyncCookies(it) }
                version++
            }
            Text(
                stringResource(R.string.sync_cookies_hint),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(start = 8.dp, bottom = 8.dp),
            )
            if (c.savedCredentials.isEmpty()) {
                Text(
                    stringResource(R.string.no_saved_passwords),
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(8.dp),
                )
            }
            for (saved in c.savedCredentials) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f).padding(start = 8.dp)) {
                        Text(
                            android.net.Uri.parse(saved.origin).host ?: saved.origin,
                            style = MaterialTheme.typography.bodyLarge,
                        )
                        Text(
                            when {
                                saved.never -> stringResource(R.string.password_never_saved)
                                saved.username.isEmpty() -> stringResource(R.string.password_no_username)
                                else -> saved.username
                            },
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    IconButton(onClick = { c.removeCredential(saved) }) {
                        Icon(
                            Icons.Filled.Delete,
                            contentDescription = stringResource(R.string.password_remove),
                        )
                    }
                }
            }

            Section(stringResource(R.string.clear_browsing_data))
            Check(stringResource(R.string.history), history) { history = it }
            Check(stringResource(R.string.clear_cookies), cookies) { cookies = it }
            Check(stringResource(R.string.clear_cache), cache) { cache = it }
            Button(
                onClick = { c.clearBrowsingData(history, cookies, cache) },
                enabled = history || cookies || cache,
                modifier = Modifier.padding(top = 8.dp),
            ) { Text(stringResource(R.string.clear_now)) }
        }
    }
}

@Composable
private fun Section(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.SemiBold,
        modifier = Modifier.padding(top = 20.dp, bottom = 6.dp),
    )
}

@Composable
private fun Check(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!checked) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = checked, onCheckedChange = null)
        Text(label, Modifier.padding(start = 8.dp, top = 10.dp, bottom = 10.dp))
    }
}
