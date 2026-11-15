package io.github.ajustor.browsingunicorns

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.mobile.BrowserState
import uniffi.mobile.ExtensionInfo
import uniffi.mobile.MobileBrowser
import uniffi.mobile.MobileException
import uniffi.mobile.SyncInfo
import uniffi.mobile.UserScriptInfo

enum class Screen { Browser, Tabs, History, Bookmarks, Sync, Extensions }

/**
 * The screen's state, refreshed from the Rust core after every action, and
 * the actions themselves. Sync rounds run on the IO dispatcher.
 */
class BrowserController(val browser: MobileBrowser, val views: WebViews, private val scope: CoroutineScope) {
    var state by mutableStateOf(browser.state())
        private set
    var sync by mutableStateOf(browser.syncInfo())
        private set
    var screen by mutableStateOf(Screen.Browser)
    var progress by mutableIntStateOf(100)
        private set
    var message by mutableStateOf<String?>(null)
    var syncing by mutableStateOf(false)
        private set
    var extensions by mutableStateOf<List<ExtensionInfo>>(emptyList())
        private set
    var installing by mutableStateOf(false)
        private set
    var userScripts by mutableStateOf<List<UserScriptInfo>>(emptyList())
        private set

    /** The extension popup shown, and its title. */
    var popup by mutableStateOf<android.webkit.WebView?>(null)
        private set
    var popupTitle by mutableStateOf("")
        private set

    private var lastSync = 0L

    init {
        views.onChange = ::refresh
        views.onProgress = { progress = it }
        views.onPanel = { popup = it }
    }

    /** Shows an extension's popup; it sees the active tab. */
    fun openPopup(id: String, name: String) {
        val url = browser.extensionPopupUrl(id) ?: return
        popupTitle = name
        views.showPanel(url)
    }

    fun closePopup() {
        views.showPanel(null)
        browser.extensionPopupClosed()
    }

    fun openOptions(id: String) {
        val url = browser.extensionOptionsUrl(id) ?: return
        open(url, newTab = true)
    }

    fun refresh() {
        state = browser.state()
    }

    /** Runs [action], refreshes, and shows its error if it fails. */
    fun act(action: MobileBrowser.() -> Unit) {
        try {
            browser.action()
        } catch (e: MobileException) {
            message = reason(e)
        }
        refresh()
    }

    fun open(url: String, newTab: Boolean = false) {
        act { open(url, newTab) }
        screen = Screen.Browser
    }

    fun goBack(): Boolean {
        val view = views.active() ?: return false
        if (!view.canGoBack()) return false
        view.goBack()
        return true
    }

    // --- Developer tools ----------------------------------------------------

    var webDebugging by mutableStateOf(views.webDebugging)
        private set

    fun toggleWebDebugging() {
        views.webDebugging = !views.webDebugging
        webDebugging = views.webDebugging
        message = if (webDebugging) {
            "Web debugging on: inspect the pages from chrome://inspect on a computer (USB debugging)"
        } else {
            "Web debugging off"
        }
    }

    // --- Extensions ---------------------------------------------------------

    fun refreshExtensions() {
        extensions = browser.extensions()
        userScripts = browser.userScripts()
    }

    /** Downloads and installs a user script from its `.user.js` address. */
    fun installUserScript(url: String) {
        if (installing) return
        installing = true
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                try {
                    "${browser.installUserScript(url)} installed: it runs from the next page load."
                } catch (e: MobileException) {
                    reason(e)
                }
            }
            installing = false
            refreshExtensions()
            message = result
        }
    }

    fun setUserScriptEnabled(id: String, enabled: Boolean) {
        act { setUserScriptEnabled(id, enabled) }
        refreshExtensions()
    }

    fun removeUserScript(id: String) {
        act { removeUserScript(id) }
        refreshExtensions()
    }

    /** Downloads and installs from a Chrome Web Store link or id. */
    fun installExtension(input: String) {
        if (installing) return
        installing = true
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                try {
                    "${browser.installExtension(input)} installed."
                } catch (e: MobileException) {
                    reason(e)
                }
            }
            installing = false
            refreshExtensions()
            message = result
        }
    }

    fun setExtensionEnabled(id: String, enabled: Boolean) {
        act { setExtensionEnabled(id, enabled) }
        refreshExtensions()
    }

    fun removeExtension(id: String) {
        act { removeExtension(id) }
        refreshExtensions()
    }

    // --- Sync ---------------------------------------------------------------

    fun refreshSync() {
        sync = browser.syncInfo()
    }

    fun syncTurnOn() {
        act { syncTurnOn() }
        syncNow()
    }

    fun syncJoin(code: String) {
        act { syncJoin(code) }
        refreshSync()
        if (sync.enabled) syncNow()
    }

    fun syncTurnOff() {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { browser.syncTurnOff() } }
            refreshSync()
        }
    }

    fun syncNow() {
        if (syncing) return
        syncing = true
        refreshSync()
        scope.launch {
            val error = withContext(Dispatchers.IO) {
                try {
                    browser.syncNow()
                    null
                } catch (e: MobileException) {
                    reason(e)
                }
            }
            lastSync = System.currentTimeMillis()
            syncing = false
            refreshSync()
            refresh()
            if (error != null && screen == Screen.Sync) message = error
        }
    }

    /**
     * While the app is in the foreground: a round 15 s after a change, and
     * at least every 5 minutes.
     */
    suspend fun syncLoop() {
        while (true) {
            if (sync.enabled && !syncing) {
                val since = System.currentTimeMillis() - lastSync
                if (since >= 5 * 60_000 || (since >= 15_000 && browser.hasUnsyncedChanges())) syncNow()
            }
            delay(5_000)
        }
    }

    companion object {
        fun reason(e: MobileException): String = when (e) {
            is MobileException.Failed -> e.reason
        }
    }
}
