package io.github.ajustor.browsingunicorns

import android.content.Intent
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

class MainActivity : ComponentActivity() {
    private lateinit var controller: BrowserController

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        val views = WebViews(this)
        val browser = MobileBrowser(
            dataDir = filesDir.absolutePath,
            deviceName = Build.MODEL ?: "Android",
            relayUrl = BuildConfig.SYNC_RELAY.ifEmpty { null },
            host = views,
            http = SyncHttpClient(),
        )
        views.browser = browser
        controller = BrowserController(browser, views, AppUpdates(this, lifecycleScope), lifecycleScope)
        controller.act { restoreSession() }
        openFromIntent(intent)

        setContent {
            BrowsingUnicornsTheme {
                App(controller)
            }
        }

        onBackPressedDispatcher.addCallback(this) {
            when {
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
                controller.syncLoop()
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openFromIntent(intent)
    }

    /** A link opened from another app. */
    private fun openFromIntent(intent: Intent?) {
        val url = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString ?: return
        controller.act { submit(url, true) }
        controller.screen = Screen.Browser
    }

    override fun onStop() {
        super.onStop()
        runCatching { controller.browser.persist() }
    }
}
