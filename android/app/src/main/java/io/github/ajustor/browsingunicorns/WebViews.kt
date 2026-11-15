package io.github.ajustor.browsingunicorns

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Message
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
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import uniffi.mobile.Interception
import uniffi.mobile.MobileBrowser
import uniffi.mobile.WebViewHost

/**
 * The engine side of the browser on Android: one WebView per tab, kept in
 * [container], only the active one visible. The Rust core decides what to
 * open and show (WebViewHost); the WebViews report back what happens.
 */
class WebViews(private val context: Context) : WebViewHost {
    val container = FrameLayout(context)
    private val views = HashMap<ULong, WebView>()

    /** Each tab's page, readable from the WebView's IO thread (blocking rules). */
    private val pages = ConcurrentHashMap<ULong, String>()

    /** The scripts injected into every page (extensions' content scripts, user scripts). */
    private var pageScripts: List<String> = emptyList()
    private val injected = HashMap<ULong, List<ScriptHandler>>()
    private val documentStart = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)

    /** The page scripts' bridge to the Rust core (their storage); guarded by a token. */
    private inner class Bridge {
        @JavascriptInterface
        fun call(message: String): String = runCatching { browser.pageMessage(message) }.getOrDefault("")
    }

    override fun setPageScripts(scripts: List<String>) {
        // Called from any thread: WebViews are touched on the main one.
        container.post {
            pageScripts = scripts
            for ((tab, view) in views) inject(tab, view)
        }
    }

    /** Replaces the scripts of [view]; they run from its next page load. */
    private fun inject(tab: ULong, view: WebView) {
        if (!documentStart) return
        injected.remove(tab)?.forEach { it.remove() }
        injected[tab] = pageScripts.map { WebViewCompat.addDocumentStartJavaScript(view, it, setOf("*")) }
    }

    lateinit var browser: MobileBrowser

    /** Called after each page event, to refresh the screen. */
    var onChange: () -> Unit = {}

    /** Called with the page's load progress (0-100). */
    var onProgress: (Int) -> Unit = {}

    fun active(): WebView? = views.values.firstOrNull { it.visibility == View.VISIBLE }

    override fun openView(tab: ULong, url: String) {
        val view = create(tab)
        views[tab] = view
        inject(tab, view)
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
        views.remove(tab)?.let {
            container.removeView(it)
            it.destroy()
        }
    }

    override fun show(tab: ULong?) {
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

    private fun blocked() = WebResourceResponse(
        "text/plain",
        "utf-8",
        403,
        "Blocked",
        emptyMap(),
        ByteArrayInputStream(ByteArray(0)),
    )

    @SuppressLint("SetJavaScriptEnabled")
    private fun create(tab: ULong): WebView = WebView(context).apply {
        settings.javaScriptEnabled = true
        settings.domStorageEnabled = true
        settings.setSupportMultipleWindows(true)
        settings.builtInZoomControls = true
        settings.displayZoomControls = false
        addJavascriptInterface(Bridge(), "BUBridge")
        webViewClient = object : WebViewClient() {
            override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
                pages[tab] = url
                // Older WebViews: the scripts run as early as the app can.
                if (!documentStart) pageScripts.forEach { view.evaluateJavascript(it, null) }
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

            // The extensions' network rules (ad and tracker blockers).
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                val headers = request.requestHeaders
                val verdict = runCatching {
                    browser.intercept(
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
                    is Interception.Block -> blocked()
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

            // target="_blank" / window.open: a throwaway WebView catches the
            // address, which opens in a new tab.
            override fun onCreateWindow(
                view: WebView,
                isDialog: Boolean,
                isUserGesture: Boolean,
                resultMsg: Message,
            ): Boolean {
                val catcher = WebView(context)
                catcher.webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(v: WebView, request: WebResourceRequest): Boolean {
                        runCatching { browser.onNewWindow(request.url.toString()) }
                        onChange()
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
}
