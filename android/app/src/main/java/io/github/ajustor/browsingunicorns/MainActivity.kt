package io.github.ajustor.browsingunicorns

import android.content.ComponentCallbacks2
import android.content.pm.PackageManager
import android.net.Uri
import android.webkit.WebChromeClient
import androidx.activity.result.contract.ActivityResultContracts
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.addCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.launch
import io.github.ajustor.browsingunicorns.ui.App
import io.github.ajustor.browsingunicorns.ui.BrowsingUnicornsTheme
import uniffi.mobile.setLanguage

class MainActivity : ComponentActivity() {
    private lateinit var controller: BrowserController

    /** A page's file picker waiting for its files. */
    private var pickedFiles: ((Array<Uri>?) -> Unit)? = null
    private val pickFiles = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data
        // Several files come in the clip data, one in the data.
        val clip = data?.clipData
        val uris = if (result.resultCode == RESULT_OK && clip != null) {
            Array(clip.itemCount) { clip.getItemAt(it).uri }
        } else {
            WebChromeClient.FileChooserParams.parseResult(result.resultCode, data)
        }
        pickedFiles?.invoke(uris)
        pickedFiles = null
    }

    /** Android's answer for the permissions a site needs. */
    private var permissionsAnswer: ((Boolean) -> Unit)? = null
    private val askPermissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
        // Coarse location is enough: fine may be refused.
        val ok = granted.filterKeys { it != android.Manifest.permission.ACCESS_FINE_LOCATION }.values.all { it }
        permissionsAnswer?.invoke(ok)
        permissionsAnswer = null
    }

    /** The downloads' answer for the storage permission (Android 9 and older). */
    private var storageAnswer: ((Boolean) -> Unit)? = null
    private val askStorage = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        storageAnswer?.invoke(granted)
        storageAnswer = null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Before the browser: its first messages already speak the app's language.
        applyLanguage()

        // Recreated (a language or font size change): the browser already
        // running is taken over.
        val core = ViewModelProvider(this)[BrowserCore::class.java]
        val (_, downloads, views, browser) = core.attach(this)
        downloads.askStorage = { done ->
            storageAnswer?.invoke(false)
            storageAnswer = done
            askStorage.launch(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)
        }
        views.chooseFiles = { intent, done ->
            pickedFiles?.invoke(null)
            pickedFiles = done
            try {
                pickFiles.launch(intent)
            } catch (_: Exception) {
                pickedFiles = null
                done(null)
            }
        }
        views.androidPermissions = { permissions, done ->
            val missing = permissions.filter {
                checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
            }
            if (missing.isEmpty()) {
                done(true)
            } else {
                permissionsAnswer?.invoke(false)
                permissionsAnswer = done
                askPermissions.launch(missing.toTypedArray())
            }
        }
        controller = BrowserController(this, browser, views, downloads, AppUpdates(this, lifecycleScope), lifecycleScope)
        if (core.fresh) controller.act { restoreSession() }
        // A recreated activity gets its first intent again: already opened.
        if (savedInstanceState == null) openFromIntent(intent)

        setContent {
            BrowsingUnicornsTheme {
                App(controller)
            }
        }

        onBackPressedDispatcher.addCallback(this) {
            when {
                controller.fullscreen != null -> controller.exitFullscreen()
                controller.finding && controller.screen == Screen.Browser -> controller.closeFind()
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
        // Off screen: the pages stop working (battery). Not in onPause: a
        // permission dialog or the other half of a split screen pauses too.
        controller.views.pause()
    }

    override fun onStart() {
        super.onStart()
        if (::controller.isInitialized) controller.views.resume()
    }
}
