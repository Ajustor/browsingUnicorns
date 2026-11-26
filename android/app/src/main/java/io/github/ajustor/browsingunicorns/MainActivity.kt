package io.github.ajustor.browsingunicorns

import android.content.ComponentCallbacks2
import android.content.Intent
import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.addCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.launch
import io.github.ajustor.browsingunicorns.ui.App
import io.github.ajustor.browsingunicorns.ui.BrowsingUnicornsTheme
import uniffi.mobile.MobileBrowser
import uniffi.mobile.setLanguage

class MainActivity : ComponentActivity() {
    private lateinit var controller: BrowserController

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Before the browser: its first messages already speak the app's language.
        applyLanguage()

        val downloads = Downloads(this)
        val views = WebViews(this, downloads)
        val browser = MobileBrowser(
            dataDir = filesDir.absolutePath,
            deviceName = Build.MODEL ?: "Android",
            relayUrl = BuildConfig.SYNC_RELAY.ifEmpty { null },
            host = views,
            http = SyncHttpClient(this),
        )
        views.browser = browser
        controller = BrowserController(this, browser, views, AppUpdates(this, lifecycleScope), lifecycleScope)
        downloads.onMessage = { controller.message = it }
        controller.act { restoreSession() }
        openFromIntent(intent)

        setContent {
            BrowsingUnicornsTheme {
                App(controller)
            }
        }

        onBackPressedDispatcher.addCallback(this) {
            when {
                controller.fullscreen != null -> controller.exitFullscreen()
                controller.screen != Screen.Browser -> controller.screen = Screen.Browser
                controller.goBack() -> {}
                else -> {
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                    isEnabled = true
                }
            }
        }

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                controller.updates.checkIfDue()
                controller.updateBlockerLists()
                controller.updateExtensions()
                launch { controller.sleepLoop() }
                controller.syncLoop()
            }
        }
    }

    /**
     * In the background with Android short of memory: every tab but the one
     * shown goes to sleep now (rather than the whole app being killed).
     */
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= ComponentCallbacks2.TRIM_MEMORY_BACKGROUND && ::controller.isInitialized) {
            controller.sleepIdleTabs(0)
        }
    }

    /**
     * A language change recreates the activity (the manifest does not handle
     * `locale`), so onCreate gives the new one; this covers a configuration
     * change handled in place all the same.
     */
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        applyLanguage()
    }

    /**
     * Gives the Rust core the language of the app's resources: the one
     * Android resolved from the system or per-app language (Android 13+)
     * among those the app has (values, values-fr), so the core's messages
     * (errors…) match the interface. Android ignores device.toml.
     */
    private fun applyLanguage() {
        setLanguage(getString(R.string.core_language_tag))
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openFromIntent(intent)
    }

    /** A link opened from another app, or text shared to this one. */
    private fun openFromIntent(intent: Intent?) {
        val input = when (intent?.action) {
            Intent.ACTION_VIEW -> intent.dataString
            Intent.ACTION_SEND -> sharedInput(intent.getStringExtra(Intent.EXTRA_TEXT))
            else -> null
        } ?: return
        controller.act { submit(input, true) }
        controller.screen = Screen.Browser
    }

    /**
     * Saves history and the normal tabs. Private tabs stay open while the
     * process lives (Android stops apps often, and swiping the app away can't
     * be told apart): they are never saved, so a new process has none, and
     * their WebView profile is deleted at start (WebViews).
     */
    override fun onStop() {
        super.onStop()
        runCatching { controller.browser.persist() }
    }
}
