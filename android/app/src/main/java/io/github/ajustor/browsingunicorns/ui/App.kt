package io.github.ajustor.browsingunicorns.ui

import android.view.ViewGroup
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Star
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import io.github.ajustor.browsingunicorns.AppUpdates
import io.github.ajustor.browsingunicorns.R
import io.github.ajustor.browsingunicorns.BrowserController
import io.github.ajustor.browsingunicorns.UpdateState
import io.github.ajustor.browsingunicorns.Screen
import uniffi.mobile.SuggestionInfo

@Composable
fun App(c: BrowserController) {
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(c.message) {
        c.message?.let {
            snackbar.showSnackbar(it)
            c.message = null
        }
    }
    // The bars reach under the status and navigation bars: each screen pads
    // itself (BrowserScreen's top block and bottom bar, the headers).
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
    ) { padding ->
        // The other screens' lists stop above the navigation bar, as before.
        val bottom = if (c.screen == Screen.Browser) Modifier else Modifier.navigationBarsPadding()
        Box(Modifier.fillMaxSize().padding(padding).imePadding().then(bottom)) {
            when (c.screen) {
                Screen.Browser -> BrowserScreen(c)
                Screen.Tabs -> TabsScreen(c)
                Screen.History -> HistoryScreen(c)
                Screen.Bookmarks -> BookmarksScreen(c)
                Screen.Sync -> SyncScreen(c)
                Screen.Extensions -> ExtensionsScreen(c)
                Screen.Settings -> SettingsScreen(c)
            }
            c.popup?.let { ExtensionPopup(c, it) }
            c.fullscreen?.let { FullscreenView(it) }
        }
    }
}

@Composable
private fun BrowserScreen(c: BrowserController) {
    Column(Modifier.fillMaxSize()) {
        val top = if (c.activePrivate) PrivateGradient else UnicornGradient
        Box(Modifier.fillMaxWidth().background(top).statusBarsPadding()) {
            AddressBar(c)
        }
        if (c.progress < 100) {
            Box(Modifier.fillMaxWidth().height(3.dp).background(Unicorn.Night)) {
                Box(
                    Modifier.fillMaxWidth(c.progress / 100f).height(3.dp).background(ProgressGradient),
                )
            }
        }
        AndroidView(
            factory = {
                // Back from another screen: the container is re-attached here.
                (c.views.container.parent as? ViewGroup)?.removeView(c.views.container)
                c.views.container
            },
            modifier = Modifier.weight(1f).fillMaxWidth(),
        )
        StoreBanner(c)
        UpdateBanner(c.updates)
        BottomBar(c)
    }
}

/** A Web Store page of an extension not installed: one tap installs it. */
@Composable
private fun StoreBanner(c: BrowserController) {
    val offer = c.storeOffer ?: return
    Surface(tonalElevation = 6.dp, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.padding(start = 16.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.store_offer, offer.name.ifEmpty { stringResource(R.string.store_offer_unnamed) }),
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { c.installExtension(offer.id) }, enabled = !c.installing) {
                Text(stringResource(if (c.installing) R.string.installing else R.string.install))
            }
        }
    }
}

/** A newer version: install it, later, or skip it. */
@Composable
private fun UpdateBanner(u: AppUpdates) {
    val (version, label) = when (val s = u.state) {
        is UpdateState.Available -> s.release.version to stringResource(R.string.install)
        is UpdateState.Ready -> s.release.version to stringResource(R.string.install)
        is UpdateState.Downloading -> s.release.version to stringResource(R.string.update_downloading, s.percent)
        else -> return
    }
    Surface(tonalElevation = 6.dp, modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.padding(start = 16.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.update_available, version),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.weight(1f),
            )
            val downloading = u.state is UpdateState.Downloading
            if (!downloading) {
                TextButton(onClick = { u.skip() }) { Text(stringResource(R.string.update_skip)) }
                TextButton(onClick = { u.later() }) { Text(stringResource(R.string.update_later)) }
            }
            TextButton(onClick = { u.install() }, enabled = !downloading) { Text(label) }
        }
    }
}

@Composable
private fun AddressBar(c: BrowserController) {
    val url = c.state.activeUrl ?: ""
    var field by remember(url) { mutableStateOf(TextFieldValue(url)) }
    val text = field.text
    var focused by remember { mutableStateOf(false) }
    // Touched: the whole address is selected, ready to be typed over. A
    // frame later: the tap that gave the focus places the cursor after it.
    LaunchedEffect(focused) {
        if (focused) field = field.copy(selection = TextRange(0, field.text.length))
    }
    val focus = LocalFocusManager.current
    var siteInfo by remember { mutableStateOf(false) }
    var shield by remember { mutableStateOf(false) }
    val host = android.net.Uri.parse(url).host ?: url
    val blocker = c.blocker
    if (shield && blocker != null) {
        ShieldDialog(
            host = host,
            info = blocker,
            onSiteChange = { c.setBlockerForSite(host, it) },
            onBlockerChange = { c.setBlocker(it) },
            onDismiss = { shield = false },
        )
    }
    if (siteInfo) {
        SiteInfoDialog(
            host = host,
            secure = c.state.activeSecure,
            certificate = c.views.active()?.certificate,
            onDismiss = { siteInfo = false },
        )
    }
    // Typing here: the page keeps off the focus until it is touched.
    DisposableEffect(c.views) {
        c.views.onPageTouched = { focus.clearFocus() }
        onDispose {
            c.views.onPageTouched = {}
            c.views.addressBarFocused = false
        }
    }
    // Typed, not the page's address: past searches and pages of the history.
    val suggestions = remember(text, focused) {
        if (focused && text != url) c.suggest(text) else emptyList()
    }
    Column {
        TextField(
            value = field,
            onValueChange = { field = it },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)
                .border(1.dp, Unicorn.Outline, RoundedCornerShape(24.dp))
                .onFocusChanged {
                    focused = it.isFocused
                    c.views.addressBarFocused = it.isFocused
                },
            singleLine = true,
            placeholder = { Text(stringResource(R.string.address_placeholder)) },
            leadingIcon = {
                if (c.activePrivate) {
                    Row(Modifier.padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(MaskIcon, contentDescription = stringResource(R.string.private_tab), modifier = Modifier.size(20.dp))
                        if (url.isNotEmpty()) SiteButton(c) { siteInfo = true }
                    }
                } else if (url.isEmpty()) {
                    Icon(Icons.Filled.Search, contentDescription = null, modifier = Modifier.size(18.dp))
                } else {
                    SiteButton(c) { siteInfo = true }
                }
            },
            trailingIcon = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (url.isNotEmpty() && blocker != null) {
                        ShieldButton(blocker) { shield = true }
                    }
                    IconButton(onClick = { c.act { reload() } }) {
                        Icon(Icons.Filled.Refresh, contentDescription = stringResource(R.string.reload))
                    }
                }
            },
            shape = RoundedCornerShape(24.dp),
            colors = TextFieldDefaults.colors(
                focusedContainerColor = Unicorn.NightDeep,
                unfocusedContainerColor = Unicorn.NightDeep,
                focusedTextColor = Unicorn.OnNight,
                unfocusedTextColor = Unicorn.OnNight,
                cursorColor = Unicorn.Cyan,
                focusedLeadingIconColor = Unicorn.OnNight,
                unfocusedLeadingIconColor = Unicorn.OnNight,
                focusedTrailingIconColor = Unicorn.OnNight,
                unfocusedTrailingIconColor = Unicorn.OnNight,
                focusedPlaceholderColor = Unicorn.OnNight.copy(alpha = 0.6f),
                unfocusedPlaceholderColor = Unicorn.OnNight.copy(alpha = 0.6f),
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = {
                c.act { submit(text, false) }
                focus.clearFocus()
            }),
        )
        for (s in suggestions) {
            SuggestionRow(s) {
                c.open(s.url)
                focus.clearFocus()
            }
        }
    }
}

/** A past search, or a page of the history with its address. */
@Composable
private fun SuggestionRow(s: SuggestionInfo, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            if (s.search) Icons.Filled.Search else HistoryIcon,
            contentDescription = stringResource(if (s.search) R.string.suggestion_search else R.string.suggestion_page),
            tint = Unicorn.OnNight.copy(alpha = 0.7f),
            modifier = Modifier.size(18.dp),
        )
        Column(Modifier.padding(start = 14.dp)) {
            Text(
                s.text.ifBlank { s.url },
                color = Unicorn.OnNight,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!s.search && s.text.isNotBlank()) {
                Text(
                    s.url,
                    color = Unicorn.OnNight.copy(alpha = 0.6f),
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** The padlock: the connection and its certificate. */
@Composable
private fun SiteButton(c: BrowserController, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(
            if (c.state.activeSecure) Icons.Filled.Lock else Icons.Filled.Info,
            contentDescription = stringResource(if (c.state.activeSecure) R.string.site_secure else R.string.site_not_secure),
            tint = if (c.state.activeSecure) Unicorn.Cyan else Unicorn.OnNight,
            modifier = Modifier.size(18.dp),
        )
    }
}

@Composable
private fun BottomBar(c: BrowserController) {
    var menu by remember { mutableStateOf(false) }
    val private = c.activePrivate
    Surface(color = if (private) Private.Bar else Unicorn.Night, contentColor = Unicorn.OnNight) {
        Row(
            Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 4.dp),
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.SpaceAround,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = { c.goBack() }, enabled = c.state.canGoBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
            }
            IconButton(onClick = { c.act { goForward() } }, enabled = c.state.canGoForward) {
                Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = stringResource(R.string.forward))
            }
            IconButton(onClick = { c.act { goHome() } }) {
                Icon(Icons.Filled.Home, contentDescription = stringResource(R.string.home))
            }
            IconButton(onClick = { c.act { toggleBookmark() } }, enabled = c.state.activeUrl != null) {
                if (c.state.bookmarked) {
                    Icon(Icons.Filled.Star, contentDescription = stringResource(R.string.remove_bookmark), tint = Unicorn.Cyan)
                } else {
                    Icon(Icons.Outlined.Star, contentDescription = stringResource(R.string.bookmark_this_page))
                }
            }
            IconButton(onClick = { c.openTabs() }) {
                Surface(
                    shape = RoundedCornerShape(6.dp),
                    border = BorderStroke(2.dp, Unicorn.OnNight),
                    color = Color.Transparent,
                    modifier = Modifier.size(24.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            c.state.tabs.count { it.private == private }.coerceAtMost(99).toString(),
                            style = MaterialTheme.typography.labelMedium,
                            maxLines = 1,
                            overflow = TextOverflow.Clip,
                        )
                    }
                }
            }
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.menu))
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.new_tab)) }, onClick = {
                        menu = false
                        c.act { openTab(null, false) }
                    })
                    if (c.state.canReopenClosedTab) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.reopen_closed_tab)) }, onClick = {
                            menu = false
                            c.act { reopenClosedTab() }
                        })
                    }
                    DropdownMenuItem(text = { Text(stringResource(R.string.history)) }, onClick = {
                        menu = false
                        c.screen = Screen.History
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.bookmarks)) }, onClick = {
                        menu = false
                        c.screen = Screen.Bookmarks
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.sync)) }, onClick = {
                        menu = false
                        c.refreshSync()
                        c.screen = Screen.Sync
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.extensions)) }, onClick = {
                        menu = false
                        c.refreshExtensions()
                        c.screen = Screen.Extensions
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.settings)) }, onClick = {
                        menu = false
                        c.screen = Screen.Settings
                    })
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.check_for_updates, c.updates.currentVersion)) },
                        onClick = {
                            menu = false
                            c.updates.check(manual = true)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.web_debugging)) },
                        trailingIcon = { Checkbox(checked = c.webDebugging, onCheckedChange = null) },
                        onClick = {
                            menu = false
                            c.toggleWebDebugging()
                        },
                    )
                }
            }
        }
    }
}

/** An extension's popup, over the browser (it sees the active tab). */
@Composable
private fun ExtensionPopup(c: BrowserController, view: android.webkit.WebView) {
    androidx.compose.ui.window.Dialog(onDismissRequest = { c.closePopup() }) {
        Surface(shape = RoundedCornerShape(16.dp), tonalElevation = 6.dp) {
            Column(Modifier.fillMaxWidth()) {
                Text(
                    c.popupTitle,
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(16.dp),
                )
                AndroidView(
                    factory = {
                        (view.parent as? ViewGroup)?.removeView(view)
                        view
                    },
                    modifier = Modifier.fillMaxWidth().height(480.dp),
                )
            }
        }
    }
}


/** A page's element in full screen (a video), over everything, the system
 *  bars hidden while it shows. */
@Composable
private fun FullscreenView(view: android.view.View) {
    val window = (androidx.compose.ui.platform.LocalContext.current as? android.app.Activity)?.window
    DisposableEffect(view) {
        val insets = window?.let { androidx.core.view.WindowCompat.getInsetsController(it, it.decorView) }
        insets?.systemBarsBehavior =
            androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        insets?.hide(androidx.core.view.WindowInsetsCompat.Type.systemBars())
        onDispose { insets?.show(androidx.core.view.WindowInsetsCompat.Type.systemBars()) }
    }
    androidx.compose.ui.viewinterop.AndroidView(
        factory = { view.also { (it.parent as? android.view.ViewGroup)?.removeView(it) } },
        modifier = Modifier.fillMaxSize().background(Color.Black),
    )
}
