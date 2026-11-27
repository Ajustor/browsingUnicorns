package io.github.ajustor.browsingunicorns

import android.content.Context
import android.os.SystemClock
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.mobile.BlockerInfo
import uniffi.mobile.BrowserState
import uniffi.mobile.CredentialChoice
import uniffi.mobile.CredentialOffer
import uniffi.mobile.ExtensionInfo
import uniffi.mobile.MobileBrowser
import uniffi.mobile.MobileException
import uniffi.mobile.SavedCredential
import uniffi.mobile.SuggestionInfo
import uniffi.mobile.SyncInfo
import uniffi.mobile.UserScriptInfo

enum class Screen { Browser, Tabs, History, Bookmarks, Sync, Extensions, Settings }

/**
 * The screen's state, refreshed from the Rust core after every action, and
 * the actions themselves. Sync rounds run on the IO dispatcher. [context]
 * gives the messages' texts, in the app's language.
 */
class BrowserController(
    private val context: Context,
    val browser: MobileBrowser,
    val views: WebViews,
    val updates: AppUpdates,
    private val scope: CoroutineScope,
) {
    var state by mutableStateOf(browser.state())

    /** A site's request (camera, location...) waiting for the user. */
    var siteAsk by mutableStateOf<SiteAsk?>(null)
        private set
    var sync by mutableStateOf(browser.syncInfo())
        private set
    var screen by mutableStateOf(Screen.Browser)

    /** The series the tabs grid shows: the private tabs, or the others. */
    var showPrivate by mutableStateOf(false)

    /** The tab shown is private (its bars turn anthracite). */
    val activePrivate: Boolean
        get() = state.tabs.firstOrNull { it.id == state.active }?.private == true

    /** Shows the tabs grid on the series of the tab shown. */
    fun openTabs() {
        views.captureActive()
        showPrivate = activePrivate
        screen = Screen.Tabs
    }
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

    /** The ad blocker on the active tab's page. */
    var blocker by mutableStateOf<BlockerInfo?>(null)
        private set

    /** A sign-in typed in a page, that the user is asked to save. */
    var credentialOffer by mutableStateOf<CredentialOffer?>(null)
        private set

    // --- Bars folded away while reading ---------------------------------------

    /** The page scrolled down: the address bar shrinks to the site's name, the bottom bar hides. */
    var barsCollapsed by mutableStateOf(false)
        private set

    /** How far the page is pulled down past its top (pixels), to reload it. */
    var pull by mutableFloatStateOf(0f)
        private set

    private val density = context.resources.displayMetrics.density

    /** Scrolled this far one way (pixels, signed) since the last change of way. */
    private var scrollRun = 0
    private var ignoreScrollUntil = 0L

    /** The shown page scrolled from [oldY] to [y]: down folds the bars, up brings them back. */
    private fun onScrolled(y: Int, oldY: Int) {
        if (y <= TOP_SLOP_DP * density) return foldBars(false)
        // The page's own size changed when the bars did: not the user's scroll.
        if (SystemClock.uptimeMillis() < ignoreScrollUntil) return
        val dy = y - oldY
        scrollRun = if (dy == 0 || (dy > 0) == (scrollRun > 0)) scrollRun + dy else dy
        val threshold = SCROLL_THRESHOLD_DP * density
        if (scrollRun > threshold) foldBars(true) else if (scrollRun < -threshold) foldBars(false)
    }

    /** Brings the bars back (a tap on the folded address bar, a new page). */
    fun expandBars() = foldBars(false)

    private fun foldBars(collapsed: Boolean) {
        scrollRun = 0
        if (barsCollapsed == collapsed) return
        barsCollapsed = collapsed
        ignoreScrollUntil = SystemClock.uptimeMillis() + 300
    }

    /** A page's element in full screen (a video), and how to leave it. */
    var fullscreen by mutableStateOf<android.view.View?>(null)
        private set
    private var leaveFullscreen: (() -> Unit)? = null

    /** Leaves the full screen of a page's element. */
    fun exitFullscreen() {
        leaveFullscreen?.invoke()
        fullscreen = null
        leaveFullscreen = null
    }

    /** Removes the history, the cookies and site data, the cache. */
    fun clearBrowsingData(history: Boolean, cookies: Boolean, cache: Boolean) {
        if (history) act { clearHistory() }
        if (cookies) {
            android.webkit.CookieManager.getInstance().removeAllCookies(null)
            android.webkit.WebStorage.getInstance().deleteAllData()
        }
        if (cache) views.clearCache()
        message = context.getString(R.string.browsing_data_cleared)
    }

    /** The extension popup shown, and its title. */
    var popup by mutableStateOf<android.webkit.WebView?>(null)
        private set
    var popupTitle by mutableStateOf("")
        private set

    private var lastSync = 0L

    init {
        views.onChange = ::refresh
        views.onProgress = {
            progress = it
            refreshBlocker()
        }
        views.onPanel = { popup = it }
        views.onFullscreen = { view, leave ->
            fullscreen = view
            leaveFullscreen = leave
        }
        views.onLinkTab = { message = context.getString(R.string.opened_in_new_tab) }
        views.askSite = { siteAsk = it }
        views.onScrolled = ::onScrolled
        views.onNavigation = ::expandBars
        views.onPull = { pull = it }
        views.onPullRefresh = { refresh() }
        // A recreated activity: what the browser already showed.
        popup = views.panel
        views.fullscreen?.let { (view, leave) ->
            fullscreen = view
            leaveFullscreen = leave
        }
        updates.onMessage = { message = it }
        refreshExtensions()
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
        val before = state.active
        state = browser.state()
        if (state.active != before) expandBars()
        credentialOffer = browser.pendingCredential()
        refreshBlocker()
    }

    // --- Passwords ------------------------------------------------------------

    fun answerCredential(choice: CredentialChoice) {
        act { answerCredential(choice) }
        refreshCredentials()
    }

    /** The saved sign-ins (and the sites never saved), for the settings. */
    var savedCredentials by mutableStateOf<List<SavedCredential>>(emptyList())
        private set

    fun refreshCredentials() {
        savedCredentials = browser.savedCredentials()
    }

    fun removeCredential(c: SavedCredential) {
        act { removeCredential(c.origin, c.username) }
        refreshCredentials()
    }

    private fun refreshBlocker() {
        blocker = state.active?.let { browser.blockerInfo(it) }
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

    /** What the address bar proposes for [input]: past searches, then pages of the history. */
    fun suggest(input: String): List<SuggestionInfo> =
        if (input.isBlank()) emptyList() else browser.suggest(input, 6u)

    /** Moves tab [id] where tab [target] is (both of the same series). */
    fun moveTab(id: ULong, target: ULong) {
        val to = state.tabs.indexOfFirst { it.id == target }
        if (to >= 0) act { moveTab(id, to.toUInt()) }
    }

    fun open(url: String, newTab: Boolean = false) {
        act { open(url, newTab) }
        screen = Screen.Browser
    }

    fun goBack(): Boolean = views.goBackActive()

    // --- Ad blocker ---------------------------------------------------------

    /** Installs the extensions' newer versions from their stores (once a day). */
    fun updateExtensions() {
        scope.launch {
            val updated = withContext(Dispatchers.IO) { runCatching { browser.updateExtensions() }.getOrDefault(0u) }
            if (updated > 0u) refreshExtensions()
        }
    }

    /** Loads the cached lists, and downloads them when due (once a day). */
    fun updateBlockerLists() {
        scope.launch {
            val changed = withContext(Dispatchers.IO) { browser.updateBlockerLists() }
            // A new engine: the scriptlets are asked again from the next page.
            if (changed) views.clearScriptlets()
            refresh()
        }
    }

    /** Turns the ad blocker on or off, then reloads the page. */
    fun setBlocker(enabled: Boolean) {
        act {
            setBlocker(enabled)
            views.clearScriptlets()
            reload()
        }
    }

    /** Turns the ad blocker on or off for [host], then reloads the page. */
    fun setBlockerForSite(host: String, enabled: Boolean) {
        act {
            setBlockerForSite(host, enabled)
            views.clearScriptlets()
            reload()
        }
    }

    // --- Developer tools ----------------------------------------------------

    var webDebugging by mutableStateOf(views.webDebugging)
        private set

    fun toggleWebDebugging() {
        views.webDebugging = !views.webDebugging
        webDebugging = views.webDebugging
        message = context.getString(if (webDebugging) R.string.web_debugging_on else R.string.web_debugging_off)
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
                    context.getString(R.string.user_script_installed, browser.installUserScript(url))
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
                    context.getString(R.string.extension_installed, browser.installExtension(input))
                } catch (e: MobileException) {
                    reason(e)
                }
            }
            installing = false
            refreshExtensions()
            message = result
        }
    }

    /**
     * The extension of the Web Store page shown, when it isn't installed yet:
     * the store's own button only works in Chrome, so the app offers its own.
     */
    val storeOffer: StoreOffer?
        get() {
            val id = storeExtensionId(state.activeUrl) ?: return null
            if (extensions.any { it.id == id }) return null
            val title = state.tabs.firstOrNull { it.id == state.active }?.title.orEmpty()
            val name = title.substringBeforeLast(" - Chrome Web Store").trim()
            return StoreOffer(id, name)
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
    /**
     * While the app is shown, every minute: the tabs unseen half an hour
     * go to sleep (their WebView destroyed, their page loaded again once
     * shown), as on the desktop.
     */
    suspend fun sleepLoop() {
        while (true) {
            delay(60_000)
            sleepIdleTabs(SLEEP_AFTER_S)
        }
    }

    /** Puts the tabs unseen `idleSeconds` to sleep (0: all but the one shown). */
    fun sleepIdleTabs(idleSeconds: Long) {
        if (browser.sleepIdleTabs(idleSeconds.toULong()) > 0u) refresh()
    }

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
        /** A tab unseen this long goes to sleep (seconds). */
        private const val SLEEP_AFTER_S = 30L * 60

        /** Scrolled this far (dp) one way, the bars fold or come back. */
        private const val SCROLL_THRESHOLD_DP = 48

        /** Closer than this (dp) to the page's top, the bars stay. */
        private const val TOP_SLOP_DP = 8

        fun reason(e: MobileException): String = when (e) {
            is MobileException.Failed -> e.reason
        }
    }
}

/** [name] is empty when the page title gives none. */
data class StoreOffer(val id: String, val name: String)

/**
 * The extension id of a Chrome Web Store or Edge Add-ons detail page
 * (`…/detail/<name>/<id>`), if [url] is one.
 */
fun storeExtensionId(url: String?): String? {
    val uri = android.net.Uri.parse(url ?: return null)
    val path = uri.pathSegments ?: return null
    val onStore = when (uri.host) {
        "chromewebstore.google.com" -> path.firstOrNull() == "detail"
        "chrome.google.com" -> path.take(2) == listOf("webstore", "detail")
        "microsoftedge.microsoft.com" -> path.take(2) == listOf("addons", "detail")
        else -> false
    }
    if (!onStore) return null
    return path.drop(1).lastOrNull { seg -> seg.length == 32 && seg.all { it in 'a'..'p' } }
}
