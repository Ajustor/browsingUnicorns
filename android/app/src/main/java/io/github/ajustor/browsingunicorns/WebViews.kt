package io.github.ajustor.browsingunicorns

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.util.Log
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.Manifest
import android.webkit.GeolocationPermissions
import android.webkit.JavascriptInterface
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebViewClient
import android.widget.FrameLayout
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlin.concurrent.thread
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.webkit.ProfileStore
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import uniffi.mobile.BackgroundPage
import uniffi.mobile.CookieInfo
import uniffi.mobile.Interception
import uniffi.mobile.MobileBrowser
import uniffi.mobile.PageTarget
import uniffi.mobile.WebViewHost

/**
 * The engine side of the browser on Android: one WebView per tab, kept in
 * [container], only the active one visible; hidden ones for the extensions'
 * background pages, and one for an extension popup. The Rust core decides
 * what to open and show (WebViewHost); the WebViews report back what happens.
 * [context] follows the activity shown (BrowserCore): the WebViews outlive
 * an activity recreated by a configuration change.
 */
class WebViews(private val context: Context, private val downloads: Downloads) : WebViewHost {
    val container = FrameLayout(context)
    private val views = HashMap<ULong, WebView>()

    /** Work for the main thread posted by [onMain] and not run yet. */
    private val queued = AtomicInteger()

    /** The main thread's queue (not the container's: it waits while detached). */
    private val main = Handler(Looper.getMainLooper())

    /**
     * Runs [work] on the main thread, where WebViews and [views] are touched:
     * now when already there with nothing queued before it, else posted (in
     * order). Rust calls the [WebViewHost] methods from whatever thread
     * called into it, the pages' bridge threads among them.
     */
    private fun onMain(work: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper() && queued.get() == 0) return work()
        queued.incrementAndGet()
        main.post {
            queued.decrementAndGet()
            work()
        }
    }

    /** Each tab's page, readable from the WebView's IO thread (blocking rules). */
    private val pages = ConcurrentHashMap<ULong, String>()

    /** Each tab's page, reduced, as last seen (the tabs grid). Memory only. */
    val thumbnails = mutableStateMapOf<ULong, ImageBitmap>()

    /** Each tab's favicon, as the page announced it. */
    val favicons = mutableStateMapOf<ULong, ImageBitmap>()

    /** Each site's icon, kept small on disk: a tab asleep shows it. */
    private val faviconDir = File(context.filesDir, "favicons")

    /** The kept icons read so far, by host (null: none). */
    private val keptIcons = HashMap<String, ImageBitmap?>()

    /**
     * [tab]'s icon: its page's, else (asleep, or loading) the one kept for
     * the site of [url].
     */
    fun favicon(tab: ULong, url: String?): ImageBitmap? {
        favicons[tab]?.let { return it }
        val host = url?.let { faviconHost(it) } ?: return null
        return keptIcons.getOrPut(host) {
            // With or without "www.": sites redirect from one to the other.
            val other = host.removePrefix("www.").let { if (it == host) "www.$host" else it }
            listOf(host, other).firstNotNullOfOrNull {
                File(faviconDir, "$it.png").takeIf(File::exists)
                    ?.let { f -> BitmapFactory.decodeFile(f.path)?.asImageBitmap() }
            }
        }
    }

    /** Keeps [icon] for [url]'s site, once (off the main thread). */
    private fun keepFavicon(url: String?, icon: Bitmap) {
        val host = url?.let { faviconHost(it) } ?: return
        val file = File(faviconDir, "$host.png")
        if (file.exists()) return
        val copy = icon.copy(Bitmap.Config.ARGB_8888, false) ?: return
        thread(name = "favicon") {
            runCatching {
                faviconDir.mkdirs()
                file.outputStream().use { copy.compress(Bitmap.CompressFormat.PNG, 100, it) }
            }
        }
    }

    /** A plain host name, to name its icon's file by; else null. */
    private fun faviconHost(url: String): String? {
        val host = android.net.Uri.parse(url).host?.lowercase() ?: return null
        return host.takeIf { h -> h.isNotEmpty() && !h.startsWith('.') && h.all { it.isLetterOrDigit() || it == '.' || it == '-' } }
    }

    /** Captures [tab]'s page at a third of its size; nothing if it can't. */
    private fun capture(tab: ULong) {
        val view = views[tab] ?: return
        if (view.width <= 0 || view.height <= 0) return
        runCatching {
            val bitmap = Bitmap.createBitmap(view.width / 3, view.height / 3, Bitmap.Config.RGB_565)
            val canvas = Canvas(bitmap)
            canvas.scale(1f / 3f, 1f / 3f)
            // What is on screen, not the page's top.
            canvas.translate(-view.scrollX.toFloat(), -view.scrollY.toFloat())
            view.draw(canvas)
            thumbnails[tab] = bitmap.asImageBitmap()
        }
    }

    /** Captures the visible tab (before showing the tabs grid). */
    fun captureActive() {
        views.entries.firstOrNull { it.value.visibility == View.VISIBLE }?.let { capture(it.key) }
    }

    /** The scripts injected into every page (extensions' content scripts, user scripts). */
    private var pageScripts: List<String> = emptyList()
    private val injected = HashMap<ULong, List<ScriptHandler>>()
    private val documentStart = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)

    /** The ad blocker's scriptlets added to each tab, by origin. */
    private val scriptlets = HashMap<ULong, HashMap<String, ScriptHandler>>()

    /** Older WebViews: each tab's scriptlets for its next page, run in onPageStarted. */
    private val pendingScriptlets = HashMap<ULong, String>()

    /**
     * Before [url] loads in [tab]: its site's scriptlets, at document start
     * for that origin only (once per origin).
     */
    private fun prepareScriptlets(tab: ULong, view: WebView, url: String) {
        val uri = android.net.Uri.parse(url)
        val scheme = uri.scheme ?: return
        val host = uri.host ?: return
        if (scheme != "http" && scheme != "https") return
        val origin = "$scheme://$host" + if (uri.port != -1) ":${uri.port}" else ""
        val added = scriptlets.getOrPut(tab) { HashMap() }
        if (origin in added) return
        val script = runCatching { browser.scriptlets(url) }.getOrDefault("")
        if (script.isEmpty()) return
        if (documentStart) {
            runCatching { WebViewCompat.addDocumentStartJavaScript(view, script, setOf(origin)) }
                .onSuccess { added[origin] = it }
                .onFailure { Log.w(TAG, "scriptlets: adding for $origin failed", it) }
        } else {
            // Best effort (after the page's first scripts); the host guard
            // makes a run on another page harmless.
            pendingScriptlets[tab] = script
        }
    }

    /** Before a step [by] in [tab]'s history: the scriptlets of the page it lands on. */
    private fun prepareHistory(tab: ULong, view: WebView, by: Int) {
        val history = view.copyBackForwardList()
        val index = history.currentIndex + by
        if (index !in 0 until history.size) return
        history.getItemAtIndex(index)?.url?.let { prepareScriptlets(tab, view, it) }
    }

    /** The shown tab goes back a page, if it can (the system Back). */
    fun goBackActive(): Boolean {
        val (tab, view) = views.entries.firstOrNull { it.value.visibility == View.VISIBLE } ?: return false
        if (!view.canGoBack()) return false
        goBack(tab)
        return true
    }

    /** The ad blocker changed: the scriptlets added so far go. */
    fun clearScriptlets() {
        for (added in scriptlets.values) added.values.forEach { it.remove() }
        scriptlets.clear()
        pendingScriptlets.clear()
    }

    private val prefs = context.getSharedPreferences("browser", Context.MODE_PRIVATE)

    /** Pages inspectable from a computer's `chrome://inspect` (USB debugging). */
    var webDebugging: Boolean = prefs.getBoolean("web_debugging", false)
        set(value) {
            field = value
            prefs.edit().putBoolean("web_debugging", value).apply()
            WebView.setWebContentsDebuggingEnabled(value)
        }

    init {
        WebView.setWebContentsDebuggingEnabled(webDebugging)
        // What private tabs left: the process ended without closing them.
        deletePrivateProfile()
    }

    /**
     * Private tabs can be opened: their WebViews get a profile of their own
     * (cookies, storage), deleted with the last of them. Without it, no
     * private tabs (they would share the normal tabs' cookies).
     */
    val privateSupported = WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)

    /** The private tabs' views. */
    private val privateTabs = HashSet<ULong>()

    /** Gives [view] the private tabs' profile; call right after creating it. */
    private fun usePrivateProfile(view: WebView) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) return
        ProfileStore.getInstance().getOrCreateProfile(PRIVATE_PROFILE)
        WebViewCompat.setProfile(view, PRIVATE_PROFILE)
    }

    /** Deletes the private tabs' cookies and storage (not while a view uses them). */
    private fun deletePrivateProfile() {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) return
        runCatching { ProfileStore.getInstance().deleteProfile(PRIVATE_PROFILE) }
    }

    /** The extensions' background pages, by extension id. */
    private val backgrounds = HashMap<String, WebView>()

    /** The extension popup shown, if any. */
    var panel: WebView? = null
        private set

    /** Called when the popup opens or closes. */
    var onPanel: (WebView?) -> Unit = {}

    /** The page scripts' bridge to the Rust core, for the WebView of [target]; guarded by a token. */
    private inner class Bridge(private val target: PageTarget) {
        @JavascriptInterface
        fun call(message: String): String {
            val answer = runCatching { browser.pageMessage(target, message) }.getOrDefault("")
            // A sign-in sent: the bar offering to save it shows.
            if (message.contains("\"credentials.submit\"")) container.post { onChange() }
            return answer
        }
    }

    private val siteCookies = SiteCookies(context)

    override fun cookies(): List<CookieInfo>? = siteCookies.all()

    override fun setCookies(cookies: List<CookieInfo>) = siteCookies.set(cookies)

    /**
     * An extension's file, when [request] asks for one (`https://<id>.bu-ext.invalid/…`).
     * Rust tells by the request's headers whether its page may have it (another
     * site only gets the manifest's web accessible resources).
     */
    private fun extensionFile(request: WebResourceRequest): WebResourceResponse? {
        val url = request.url.toString()
        val host = request.url.host ?: return null
        if (!host.endsWith(".bu-ext.invalid")) return null
        val headers = request.requestHeaders.orEmpty().mapKeys { it.key.lowercase() }
        val file = runCatching {
            browser.extensionResource(url, headers["origin"], headers["referer"], headers["sec-fetch-site"])
        }.getOrNull()
            ?: return WebResourceResponse("text/plain", "utf-8", 404, "Not found", emptyMap(), ByteArrayInputStream(ByteArray(0)))
        return WebResourceResponse(
            file.mimeType,
            "utf-8",
            200,
            "OK",
            if (file.cors) mapOf("Access-Control-Allow-Origin" to "*") else emptyMap(),
            ByteArrayInputStream(file.data),
        )
    }

    override fun postToPage(target: PageTarget, script: String) {
        onMain {
            val view = when (target) {
                is PageTarget.Tab -> views[target.tab]
                is PageTarget.Background -> backgrounds[target.extension]
                is PageTarget.Panel -> panel
            }
            view?.evaluateJavascript(script, null)
        }
    }

    /** The version each background page runs: an update restarts it. */
    private val backgroundVersions = HashMap<String, String>()

    override fun setBackgrounds(pages: List<BackgroundPage>) {
        onMain {
            val wanted = pages.associate { it.extension to it.url }
            val versions = pages.associate { it.extension to it.version }
            for (id in backgrounds.keys.filter { it !in wanted || backgroundVersions[it] != versions[it] }) {
                backgrounds.remove(id)?.let {
                    container.removeView(it)
                    it.destroy()
                }
                backgroundVersions.remove(id)
            }
            for ((id, url) in wanted) {
                if (id in backgrounds) continue
                versions[id]?.let { backgroundVersions[id] = it }
                val view = extensionView(PageTarget.Background(id))
                // Attached (scripts and timers run) but out of sight.
                view.visibility = View.INVISIBLE
                container.addView(view, FrameLayout.LayoutParams(1, 1))
                if (offScreen) view.onPause() else resumeTimers(view)
                backgrounds[id] = view
                view.loadUrl(url)
            }
        }
    }

    override fun showPanel(url: String?) {
        onMain {
            panel?.destroy()
            panel = url?.let { extensionView(PageTarget.Panel).apply { loadUrl(it) } }
            onPanel(panel)
        }
    }

    /** A WebView for an extension's own pages (background, popup). */
    @SuppressLint("SetJavaScriptEnabled")
    private fun extensionView(target: PageTarget): WebView = WebView(context).apply {
        settings.javaScriptEnabled = true
        settings.domStorageEnabled = true
        addJavascriptInterface(Bridge(target), "BUBridge")
        webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                extensionFile(request)

            // Its renderer died: without this, the whole app would go with it.
            override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                (view.parent as? ViewGroup)?.removeView(view)
                view.destroy()
                return true
            }
        }
    }

    override fun setPageScripts(scripts: List<String>) {
        onMain {
            pageScripts = scripts
            for ((tab, view) in views) if (tab !in privateTabs) inject(tab, view)
        }
    }

    /** Replaces the scripts of [view]; they run from its next page load. */
    private fun inject(tab: ULong, view: WebView) {
        if (!documentStart) return
        injected.remove(tab)?.forEach { it.remove() }
        injected[tab] = pageScripts.map { WebViewCompat.addDocumentStartJavaScript(view, it, setOf("*")) }
    }

    lateinit var browser: MobileBrowser

    /**
     * The address bar is being typed in: pages can't take the focus (an
     * autofocus field, a script calling focus(), a background page loading).
     */
    var addressBarFocused: Boolean = false
        set(value) {
            field = value
            container.descendantFocusability =
                if (value) ViewGroup.FOCUS_BLOCK_DESCENDANTS else ViewGroup.FOCUS_AFTER_DESCENDANTS
        }

    /** Called when the page is touched: the address bar gives the focus back. */
    var onPageTouched: () -> Unit = {}

    /** Called after each page event, to refresh the screen. */
    var onChange: () -> Unit = {}

    /** A link opened in a new tab, behind the one shown. */
    var onLinkTab: () -> Unit = {}

    /**
     * Opens Android's file picker for a page's `<input type="file">` and
     * gives back the files chosen (null: none). The activity sets it.
     */
    var chooseFiles: (Intent, (Array<Uri>?) -> Unit) -> Unit = { _, done -> done(null) }

    /** Asks Android for [permissions]; true when all were granted. The activity sets it. */
    var androidPermissions: (Array<String>, (Boolean) -> Unit) -> Unit = { _, done -> done(false) }

    /** Shows (or, null, takes away) the question to the user about a site's request. */
    var askSite: (SiteAsk?) -> Unit = {}

    /** The user's answers, by site and need, for this run. */
    private val siteAnswers = HashMap<Pair<String, SiteNeed>, Boolean>()

    /**
     * Whether [host] may use [needs]: the answers already given, else the
     * user is asked, then Android (its own permissions) when allowed.
     */
    private fun decide(host: String, needs: List<SiteNeed>, done: (Boolean) -> Unit) {
        val known = needs.map { siteAnswers[host to it] }
        if (known.all { it == true }) return androidPermissions(needs.flatMap { it.android }.toTypedArray(), done)
        if (known.any { it == false }) return done(false)
        askSite(
            SiteAsk(host, needs) { yes ->
                askSite(null)
                needs.forEach { siteAnswers[host to it] = yes }
                if (!yes) {
                    done(false)
                } else {
                    androidPermissions(needs.flatMap { it.android }.toTypedArray(), done)
                }
            },
        )
    }

    /** Called with the page's load progress (0-100). */
    var onProgress: (Int) -> Unit = {}

    /** A page shows an element in full screen (a video), with how to leave
     *  it; `null` when it left. */
    var onFullscreen: (View?, (() -> Unit)?) -> Unit = { _, _ -> }

    /** The element in full screen now, and how to leave it (for a new activity). */
    var fullscreen: Pair<View, () -> Unit>? = null
        private set

    /** The shown page scrolled from [oldY] to [y] (pixels). */
    var onScrolled: (y: Int, oldY: Int) -> Unit = { _, _ -> }

    /** The shown tab started loading another page. */
    var onNavigation: () -> Unit = {}

    /** The shown page is pulled down from its top by that many pixels (0: let go). */
    var onPull: (Float) -> Unit = {}

    /** The pull went far enough: the page reloads. */
    var onPullRefresh: () -> Unit = {}

    /** How far a page is pulled down to reload, in pixels. */
    val pullDistance get() = 96f * context.resources.displayMetrics.density

    /**
     * A tab's WebView, which reports its scrolling (the bars fold away) and
     * its pulls past the top (reload).
     */
    @SuppressLint("ViewConstructor")
    private inner class PageView(private val tab: ULong) : WebView(context) {
        /** The finger moves down. */
        private var down = false
        private var lastY = 0f

        /** The page hit its top while the finger moved down. */
        private var atTop = false
        private var pullFrom = Float.NaN

        override fun onScrollChanged(l: Int, t: Int, oldl: Int, oldt: Int) {
            super.onScrollChanged(l, t, oldl, oldt)
            if (visibility == View.VISIBLE) onScrolled(t, oldt)
        }

        // The page (or a scroller of it, through scroll chaining) could not
        // scroll further up: what is pulled now pulls the page itself.
        override fun onOverScrolled(scrollX: Int, scrollY: Int, clampedX: Boolean, clampedY: Boolean) {
            super.onOverScrolled(scrollX, scrollY, clampedX, clampedY)
            if (clampedY && scrollY == 0 && down) atTop = true
        }

        /** Follows the finger for the pull to refresh; never takes the touch. */
        fun track(event: MotionEvent) {
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    lastY = event.y
                    down = false
                    atTop = false
                    pullFrom = Float.NaN
                }
                MotionEvent.ACTION_MOVE -> {
                    if (event.pointerCount > 1) return endPull(false)
                    down = event.y > lastY
                    lastY = event.y
                    if (pullFrom.isNaN() && atTop && down && !canScrollVertically(-1)) pullFrom = event.y
                    if (!pullFrom.isNaN()) {
                        val pulled = event.y - pullFrom
                        if (pulled < 0) endPull(false) else onPull(pulled)
                    }
                }
                MotionEvent.ACTION_UP -> endPull(!pullFrom.isNaN() && event.y - pullFrom >= pullDistance)
                MotionEvent.ACTION_CANCEL -> endPull(false)
            }
        }

        private fun endPull(refresh: Boolean) {
            if (pullFrom.isNaN()) return
            pullFrom = Float.NaN
            atTop = false
            onPull(0f)
            if (refresh) {
                reload(tab)
                onPullRefresh()
            }
        }
    }

    /**
     * The browser ends (its activity finished for good): every WebView goes,
     * and with the last private one their profile.
     */
    fun destroy() {
        val all = views.values + backgrounds.values + listOfNotNull(panel)
        views.clear()
        backgrounds.clear()
        backgroundVersions.clear()
        panel = null
        fullscreen = null
        container.removeAllViews()
        all.forEach { it.destroy() }
        privateTabs.clear()
        main.post { deletePrivateProfile() }
    }

    /** The app is off screen: the pages are paused. */
    private var offScreen = false

    /** The JavaScript timers of every WebView are paused (one switch for all). */
    private var timersPaused = false

    /**
     * The app left the screen: every page pauses, and the JavaScript timers
     * of all of them (the extensions' background pages too) stop, for the
     * battery.
     */
    fun pause() {
        offScreen = true
        val all = views.values + backgrounds.values + listOfNotNull(panel)
        all.forEach { it.onPause() }
        all.firstOrNull()?.let {
            it.pauseTimers()
            timersPaused = true
        }
    }

    /** Back on screen: the timers, the shown page and the hidden ones' work resume. */
    fun resume() {
        offScreen = false
        resumeTimers()
        (backgrounds.values + listOfNotNull(panel, active())).forEach { it.onResume() }
    }

    /** Restarts the timers if they were paused, through [view] or any WebView. */
    private fun resumeTimers(view: WebView? = null) {
        if (!timersPaused) return
        val any = view ?: (views.values + backgrounds.values + listOfNotNull(panel)).firstOrNull() ?: return
        any.resumeTimers()
        timersPaused = false
    }

    /** Empties the cache the pages share. */
    fun clearCache() {
        views.values.firstOrNull()?.clearCache(true)
    }

    fun active(): WebView? = views.values.firstOrNull { it.visibility == View.VISIBLE }

    override fun openView(tab: ULong, url: String, private: Boolean) = onMain {
        val view = create(tab, private)
        views[tab] = view
        // No extension or user script in private tabs.
        if (private) privateTabs += tab else inject(tab, view)
        view.visibility = View.GONE
        // Its work waits until it is shown.
        view.onPause()
        if (!offScreen) resumeTimers(view)
        container.addView(
            view,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        prepareScriptlets(tab, view, url)
        view.loadUrl(url)
    }

    override fun closeView(tab: ULong) = onMain {
        pages.remove(tab)
        injected.remove(tab)
        scriptlets.remove(tab)
        pendingScriptlets.remove(tab)
        thumbnails.remove(tab)
        favicons.remove(tab)
        views.remove(tab)?.let {
            container.removeView(it)
            it.destroy()
        }
        // The last private tab: its cookies and storage go (once the view is gone).
        if (privateTabs.remove(tab) && privateTabs.isEmpty()) main.post { deletePrivateProfile() }
    }

    override fun show(tab: ULong?) = onMain {
        captureActive()
        for ((id, view) in views) {
            val shown = id == tab
            if (shown == (view.visibility == View.VISIBLE)) continue
            view.visibility = if (shown) View.VISIBLE else View.GONE
            // A hidden tab's animations and work pause until it shows again.
            if (shown && !offScreen) view.onResume() else if (!shown) view.onPause()
        }
    }

    override fun navigate(tab: ULong, url: String) = onMain {
        val view = views[tab] ?: return@onMain
        prepareScriptlets(tab, view, url)
        view.loadUrl(url)
    }

    // History steps bypass shouldOverrideUrlLoading: the scriptlets first.
    override fun goBack(tab: ULong) = onMain {
        val view = views[tab] ?: return@onMain
        prepareHistory(tab, view, -1)
        view.goBack()
    }

    override fun goForward(tab: ULong) = onMain {
        val view = views[tab] ?: return@onMain
        prepareHistory(tab, view, 1)
        view.goForward()
    }

    override fun reload(tab: ULong) = onMain {
        val view = views[tab] ?: return@onMain
        // After a blocker change the scriptlets were cleared: back for this page.
        view.url?.let { prepareScriptlets(tab, view, it) }
        view.reload()
    }

    /** A blocked script gets an empty one rather than an error, as uBlock
     *  Origin does: loaders (RequireJS) fail every module after one that
     *  does not load, and the page breaks (a cookie banner that can't be
     *  closed). */
    private fun blocked(script: Boolean = false) = if (script) {
        WebResourceResponse(
            "application/javascript",
            "utf-8",
            200,
            "OK",
            mapOf("Access-Control-Allow-Origin" to "*"),
            ByteArrayInputStream(ByteArray(0)),
        )
    } else {
        WebResourceResponse(
            "text/plain",
            "utf-8",
            403,
            "Blocked",
            emptyMap(),
            ByteArrayInputStream(ByteArray(0)),
        )
    }

    @SuppressLint("SetJavaScriptEnabled", "ClickableViewAccessibility")
    private fun create(tab: ULong, private: Boolean): WebView = PageView(tab).apply {
        if (private) usePrivateProfile(this)
        setOnTouchListener { _, event ->
            if (event.actionMasked == MotionEvent.ACTION_DOWN) onPageTouched()
            track(event)
            false
        }
        settings.javaScriptEnabled = true
        settings.domStorageEnabled = true
        settings.setSupportMultipleWindows(true)
        settings.builtInZoomControls = true
        settings.displayZoomControls = false
        addJavascriptInterface(Bridge(PageTarget.Tab(tab)), "BUBridge")
        addJavascriptInterface(downloads.Bridge(), "BUDownloads")
        setDownloadListener { url, userAgent, contentDisposition, mimeType, _ ->
            downloads.start(this, url, userAgent, contentDisposition, mimeType, pages[tab])
        }
        webViewClient = object : WebViewClient() {
            override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
                pages[tab] = url
                // Older WebViews: the scripts run as early as the app can.
                if (!documentStart && !private) pageScripts.forEach { view.evaluateJavascript(it, null) }
                pendingScriptlets.remove(tab)?.let { view.evaluateJavascript(it, null) }
                browser.onLoadStarted(tab, url)
                if (view.visibility == View.VISIBLE) onNavigation()
                onChange()
            }

            override fun onPageFinished(view: WebView, url: String) {
                browser.onLoadFinished(tab, url)
                onChange()
            }

            // Also catches history.pushState in single-page apps.
            override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
                browser.onLoadFinished(tab, url)
                onChange()
            }

            // The extensions' network rules, then the built-in ad blocker.
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                extensionFile(request)?.let { return it }
                val headers = request.requestHeaders
                val verdict = runCatching {
                    browser.intercept(
                        tab,
                        request.url.toString(),
                        pages[tab],
                        request.isForMainFrame,
                        request.method ?: "GET",
                        headers["Accept"] ?: headers["accept"],
                        headers["Sec-Fetch-Dest"] ?: headers["sec-fetch-dest"],
                    )
                }.getOrDefault(Interception.Allow)
                return when (verdict) {
                    is Interception.Allow -> null
                    is Interception.Block -> {
                        val dest = headers["Sec-Fetch-Dest"] ?: headers["sec-fetch-dest"]
                        val path = request.url.path ?: ""
                        blocked(script = dest == "script" || (dest == null && path.endsWith(".js")))
                    }
                    is Interception.Serve -> runCatching {
                        WebResourceResponse(verdict.mimeType, "utf-8", FileInputStream(File(verdict.path)))
                    }.getOrElse { blocked() }
                }
            }

            // The page's renderer died (crashed, or killed by Android short
            // of memory): without this, the whole app would go with it. Rust
            // closes the view (closeView) and the tab waits to be reloaded.
            override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                browser.onPageCrashed(tab)
                onChange()
                return true
            }

            // mailto:, tel:, intent:... go to the app that handles them.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                if (request.isForMainFrame) prepareScriptlets(tab, view, request.url.toString())
                val scheme = request.url.scheme ?: return false
                if (scheme in setOf("http", "https", "about", "file", "data", "javascript")) return false
                try {
                    context.startActivity(
                        Intent(Intent.ACTION_VIEW, request.url).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                } catch (_: ActivityNotFoundException) {
                }
                return true
            }
        }
        webChromeClient = object : WebChromeClient() {
            // <input type="file">: Android's picker.
            override fun onShowFileChooser(
                view: WebView,
                callback: ValueCallback<Array<Uri>>,
                params: FileChooserParams,
            ): Boolean {
                val intent = try {
                    params.createIntent()
                } catch (_: Exception) {
                    callback.onReceiveValue(null)
                    return true
                }
                if (params.mode == FileChooserParams.MODE_OPEN_MULTIPLE) {
                    intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                }
                chooseFiles(intent) { callback.onReceiveValue(it) }
                return true
            }

            // Camera, microphone: the user decides, site by site; protected
            // media (DRM) goes through, as in other browsers.
            override fun onPermissionRequest(request: PermissionRequest) {
                val resources = request.resources.toSet()
                val needs = buildList {
                    if (PermissionRequest.RESOURCE_VIDEO_CAPTURE in resources) add(SiteNeed.Camera)
                    if (PermissionRequest.RESOURCE_AUDIO_CAPTURE in resources) add(SiteNeed.Microphone)
                }
                val media = PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID
                if (needs.isEmpty()) {
                    if (media in resources) request.grant(arrayOf(media)) else request.deny()
                    return
                }
                decide(request.origin.host ?: "", needs) { allowed ->
                    if (!allowed) return@decide request.deny()
                    val granted = resources.filter {
                        it == PermissionRequest.RESOURCE_VIDEO_CAPTURE ||
                            it == PermissionRequest.RESOURCE_AUDIO_CAPTURE || it == media
                    }
                    request.grant(granted.toTypedArray())
                }
            }

            override fun onPermissionRequestCanceled(request: PermissionRequest) = askSite(null)

            override fun onGeolocationPermissionsShowPrompt(
                origin: String,
                callback: GeolocationPermissions.Callback,
            ) {
                val host = Uri.parse(origin).host ?: origin
                decide(host, listOf(SiteNeed.Location)) { callback.invoke(origin, it, false) }
            }

            override fun onGeolocationPermissionsHidePrompt() = askSite(null)

            override fun onReceivedTitle(view: WebView, title: String?) {
                browser.onTitleChanged(tab, title ?: "")
                onChange()
            }

            override fun onProgressChanged(view: WebView, newProgress: Int) {
                if (view.visibility == View.VISIBLE) onProgress(newProgress)
            }

            // A video's full screen button: its view over everything.
            override fun onShowCustomView(view: View, callback: CustomViewCallback) {
                val leave = { callback.onCustomViewHidden() }
                fullscreen = view to leave
                onFullscreen(view, leave)
            }

            override fun onHideCustomView() {
                fullscreen = null
                onFullscreen(null, null)
            }

            override fun onReceivedIcon(view: WebView, icon: Bitmap?) {
                icon?.let {
                    favicons[tab] = it.asImageBitmap()
                    if (tab !in privateTabs) keepFavicon(view.url, it)
                }
            }

            // target="_blank" / window.open: a throwaway WebView catches the
            // address, which opens in a new tab next to this one, this one
            // staying shown.
            override fun onCreateWindow(
                view: WebView,
                isDialog: Boolean,
                isUserGesture: Boolean,
                resultMsg: Message,
            ): Boolean {
                val catcher = WebView(context)
                if (private) usePrivateProfile(catcher)
                catcher.webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(v: WebView, request: WebResourceRequest): Boolean {
                        runCatching { browser.onNewWindow(tab, request.url.toString()) }
                        onChange()
                        onLinkTab()
                        v.destroy()
                        return true
                    }
                }
                (resultMsg.obj as WebView.WebViewTransport).webView = catcher
                resultMsg.sendToTarget()
                return true
            }
        }
    }

    companion object {
        /** The WebView profile of the private tabs. */
        const val PRIVATE_PROFILE = "bu-private"

        private const val TAG = "WebViews"
    }
}

/** What a site may ask to use, and the Android permissions it takes. */
enum class SiteNeed(val android: List<String>) {
    Camera(listOf(Manifest.permission.CAMERA)),
    Microphone(listOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.MODIFY_AUDIO_SETTINGS)),
    Location(listOf(Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION)),
}

/** A site's request, waiting for the user: [answer] with their choice. */
class SiteAsk(val host: String, val needs: List<SiteNeed>, val answer: (Boolean) -> Unit)
