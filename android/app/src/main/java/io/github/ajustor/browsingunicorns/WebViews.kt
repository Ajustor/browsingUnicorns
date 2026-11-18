package io.github.ajustor.browsingunicorns

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Message
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream
import java.util.concurrent.ConcurrentHashMap
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.webkit.ProfileStore
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import uniffi.mobile.BackgroundPage
import uniffi.mobile.Interception
import uniffi.mobile.MobileBrowser
import uniffi.mobile.PageTarget
import uniffi.mobile.WebViewHost

/**
 * The engine side of the browser on Android: one WebView per tab, kept in
 * [container], only the active one visible; hidden ones for the extensions'
 * background pages, and one for an extension popup. The Rust core decides
 * what to open and show (WebViewHost); the WebViews report back what happens.
 */
class WebViews(private val context: Context, private val downloads: Downloads) : WebViewHost {
    val container = FrameLayout(context)
    private val views = HashMap<ULong, WebView>()

    /** Each tab's page, readable from the WebView's IO thread (blocking rules). */
    private val pages = ConcurrentHashMap<ULong, String>()

    /** Each tab's page, reduced, as last seen (the tabs grid). Memory only. */
    val thumbnails = mutableStateMapOf<ULong, ImageBitmap>()

    /** Each tab's favicon, as the page announced it. */
    val favicons = mutableStateMapOf<ULong, ImageBitmap>()

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
        fun call(message: String): String = runCatching { browser.pageMessage(target, message) }.getOrDefault("")
    }

    /** An extension's file, when [url] is one (`https://<id>.bu-ext.invalid/…`). */
    private fun extensionFile(url: String): WebResourceResponse? {
        val host = android.net.Uri.parse(url).host ?: return null
        if (!host.endsWith(".bu-ext.invalid")) return null
        val file = runCatching { browser.extensionResource(url) }.getOrNull()
            ?: return WebResourceResponse("text/plain", "utf-8", 404, "Not found", emptyMap(), ByteArrayInputStream(ByteArray(0)))
        return WebResourceResponse(
            file.mimeType,
            "utf-8",
            200,
            "OK",
            mapOf("Access-Control-Allow-Origin" to "*"),
            ByteArrayInputStream(file.data),
        )
    }

    override fun postToPage(target: PageTarget, script: String) {
        container.post {
            val view = when (target) {
                is PageTarget.Tab -> views[target.tab]
                is PageTarget.Background -> backgrounds[target.extension]
                is PageTarget.Panel -> panel
            }
            view?.evaluateJavascript(script, null)
        }
    }

    override fun setBackgrounds(pages: List<BackgroundPage>) {
        container.post {
            val wanted = pages.associate { it.extension to it.url }
            for (id in backgrounds.keys.filter { it !in wanted }) {
                backgrounds.remove(id)?.let {
                    container.removeView(it)
                    it.destroy()
                }
            }
            for ((id, url) in wanted) {
                if (id in backgrounds) continue
                val view = extensionView(PageTarget.Background(id))
                // Attached (scripts and timers run) but out of sight.
                view.visibility = View.INVISIBLE
                container.addView(view, FrameLayout.LayoutParams(1, 1))
                backgrounds[id] = view
                view.loadUrl(url)
            }
        }
    }

    override fun showPanel(url: String?) {
        container.post {
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
                extensionFile(request.url.toString())
        }
    }

    override fun setPageScripts(scripts: List<String>) {
        // Called from any thread: WebViews are touched on the main one.
        container.post {
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

    /** Called with the page's load progress (0-100). */
    var onProgress: (Int) -> Unit = {}

    /** A page shows an element in full screen (a video), with how to leave
     *  it; `null` when it left. */
    var onFullscreen: (View?, (() -> Unit)?) -> Unit = { _, _ -> }

    /** Empties the cache the pages share. */
    fun clearCache() {
        views.values.firstOrNull()?.clearCache(true)
    }

    fun active(): WebView? = views.values.firstOrNull { it.visibility == View.VISIBLE }

    override fun openView(tab: ULong, url: String, private: Boolean) {
        val view = create(tab, private)
        views[tab] = view
        // No extension or user script in private tabs.
        if (private) privateTabs += tab else inject(tab, view)
        view.visibility = View.GONE
        container.addView(
            view,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        view.loadUrl(url)
    }

    override fun closeView(tab: ULong) {
        pages.remove(tab)
        injected.remove(tab)
        thumbnails.remove(tab)
        favicons.remove(tab)
        views.remove(tab)?.let {
            container.removeView(it)
            it.destroy()
        }
        // The last private tab: its cookies and storage go (once the view is gone).
        if (privateTabs.remove(tab) && privateTabs.isEmpty()) container.post { deletePrivateProfile() }
    }

    override fun show(tab: ULong?) {
        captureActive()
        for ((id, view) in views) {
            view.visibility = if (id == tab) View.VISIBLE else View.GONE
        }
    }

    override fun navigate(tab: ULong, url: String) {
        views[tab]?.loadUrl(url)
    }

    override fun goBack(tab: ULong) {
        views[tab]?.goBack()
    }

    override fun goForward(tab: ULong) {
        views[tab]?.goForward()
    }

    override fun reload(tab: ULong) {
        views[tab]?.reload()
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
    private fun create(tab: ULong, private: Boolean): WebView = WebView(context).apply {
        if (private) usePrivateProfile(this)
        setOnTouchListener { _, event ->
            if (event.actionMasked == MotionEvent.ACTION_DOWN) onPageTouched()
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
                browser.onLoadStarted(tab, url)
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
                extensionFile(request.url.toString())?.let { return it }
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

            // mailto:, tel:, intent:... go to the app that handles them.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
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
            override fun onReceivedTitle(view: WebView, title: String?) {
                browser.onTitleChanged(tab, title ?: "")
                onChange()
            }

            override fun onProgressChanged(view: WebView, newProgress: Int) {
                if (view.visibility == View.VISIBLE) onProgress(newProgress)
            }

            // A video's full screen button: its view over everything.
            override fun onShowCustomView(view: View, callback: CustomViewCallback) {
                onFullscreen(view) { callback.onCustomViewHidden() }
            }

            override fun onHideCustomView() {
                onFullscreen(null, null)
            }

            override fun onReceivedIcon(view: WebView, icon: Bitmap?) {
                icon?.let { favicons[tab] = it.asImageBitmap() }
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
    }
}
