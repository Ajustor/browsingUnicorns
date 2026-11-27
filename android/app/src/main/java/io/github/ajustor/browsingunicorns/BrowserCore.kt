package io.github.ajustor.browsingunicorns

import android.app.Activity
import android.content.MutableContextWrapper
import android.os.Build
import androidx.lifecycle.ViewModel
import uniffi.mobile.MobileBrowser

/**
 * The browser that outlives its activity: the Rust core, the WebViews and
 * the downloads. A configuration change the activity doesn't handle in place
 * (language, font size, density) recreates it; the new one takes these over
 * rather than opening the profile a second time. They end with the activity
 * finishing for good ([onCleared]).
 */
class BrowserCore : ViewModel() {
    /** What a new activity takes over. */
    data class Parts(
        /** The activity shown, under the WebViews and downloads. */
        val context: MutableContextWrapper,
        val downloads: Downloads,
        val views: WebViews,
        val browser: MobileBrowser,
    )

    private var parts: Parts? = null

    /** True when [attach] made the browser (rather than taking it over). */
    var fresh = false
        private set

    /** The browser, made for [activity] or handed over to it. */
    fun attach(activity: Activity): Parts {
        parts?.let {
            fresh = false
            it.context.baseContext = activity
            return it
        }
        val context = MutableContextWrapper(activity)
        val downloads = Downloads(context)
        val views = WebViews(context, downloads)
        val browser = MobileBrowser(
            dataDir = activity.filesDir.absolutePath,
            deviceName = Build.MODEL ?: "Android",
            relayUrl = BuildConfig.SYNC_RELAY.ifEmpty { null },
            host = views,
            http = SyncHttpClient(context),
        )
        views.browser = browser
        fresh = true
        return Parts(context, downloads, views, browser).also { parts = it }
    }

    /**
     * The activity finished: the WebViews go, then the core (the Rust side
     * holds the WebViews, which hold it: destroying it breaks the cycle).
     */
    override fun onCleared() {
        val p = parts ?: return
        parts = null
        runCatching { p.browser.persist() }
        p.views.destroy()
        p.downloads.close()
        p.context.baseContext = p.context.applicationContext
        p.browser.destroy()
    }
}
