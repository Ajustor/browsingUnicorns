package io.github.ajustor.browsingunicorns

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** A newer release, as `latest.json` (on GitHub Pages) describes it. */
data class Release(val version: String, val notes: String, val apkUrl: String, val sha256: String)

sealed interface UpdateState {
    data object Idle : UpdateState
    data object Checking : UpdateState
    data class Available(val release: Release) : UpdateState
    data class Downloading(val release: Release, val percent: Int) : UpdateState
    /** Downloaded and checked: the system installer takes over. */
    data class Ready(val release: Release, val apk: File) : UpdateState
}

/**
 * Self-update, like the desktop app: a daily look at the release manifest,
 * a banner when a newer version exists, the signed APK downloaded and its
 * SHA-256 checked, then Android's installer (same signing key: an update).
 */
class AppUpdates(private val context: Context, private val scope: CoroutineScope) {
    var state by mutableStateOf<UpdateState>(UpdateState.Idle)
        private set

    /** Short messages for the user (up to date, errors). */
    var onMessage: (String) -> Unit = {}

    private val prefs = context.getSharedPreferences("updates", Context.MODE_PRIVATE)
    private val dir = File(context.cacheDir, "updates")

    val currentVersion: String = BuildConfig.VERSION_NAME

    /** Checks if the last check is a day old (at start, back to the app). */
    fun checkIfDue() {
        val last = prefs.getLong("last_check", 0L)
        if (System.currentTimeMillis() - last >= DAY_MS) check(manual = false)
    }

    /** `manual`: asked from the menu, so says when there is nothing new. */
    fun check(manual: Boolean) {
        if (state is UpdateState.Checking || state is UpdateState.Downloading) return
        state = UpdateState.Checking
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { fetchLatest() } }
            prefs.edit().putLong("last_check", System.currentTimeMillis()).apply()
            val release = result.getOrElse {
                Log.w(TAG, "update check failed", it)
                state = UpdateState.Idle
                if (manual) onMessage(context.getString(R.string.update_check_failed, it.message))
                return@launch
            }
            val skipped = prefs.getString("skipped", null)
            state = when {
                release == null || !isNewer(release.version, currentVersion) -> {
                    if (manual) onMessage(context.getString(R.string.up_to_date, currentVersion))
                    UpdateState.Idle
                }
                !manual && release.version == skipped -> UpdateState.Idle
                else -> UpdateState.Available(release)
            }
        }
    }

    fun install() {
        when (val s = state) {
            is UpdateState.Available -> download(s.release)
            is UpdateState.Ready -> launchInstaller(s.apk)
            else -> {}
        }
    }

    fun later() {
        if (state is UpdateState.Available || state is UpdateState.Ready) state = UpdateState.Idle
    }

    fun skip() {
        val release = when (val s = state) {
            is UpdateState.Available -> s.release
            is UpdateState.Ready -> s.release
            else -> return
        }
        prefs.edit().putString("skipped", release.version).apply()
        state = UpdateState.Idle
    }

    private fun fetchLatest(): Release? {
        val json = JSONObject(get(MANIFEST_URL).use { it.readBytes().decodeToString() })
        val assets = json.getJSONArray("assets")
        for (i in 0 until assets.length()) {
            val asset = assets.getJSONObject(i)
            if (asset.getString("name") == APK_NAME) {
                return Release(
                    version = json.getString("version"),
                    notes = json.optString("notes"),
                    apkUrl = asset.getString("url"),
                    sha256 = asset.getString("sha256").lowercase(),
                )
            }
        }
        return null
    }

    private fun download(release: Release) {
        state = UpdateState.Downloading(release, 0)
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    dir.mkdirs()
                    dir.listFiles()?.forEach { it.delete() }
                    val apk = File(dir, "browsingUnicorns-${release.version}.apk")
                    val connection = open(release.apkUrl)
                    val total = connection.contentLengthLong
                    val digest = MessageDigest.getInstance("SHA-256")
                    connection.inputStream.use { input ->
                        apk.outputStream().use { output ->
                            val buffer = ByteArray(64 * 1024)
                            var done = 0L
                            var shown = -1
                            while (true) {
                                val n = input.read(buffer)
                                if (n < 0) break
                                output.write(buffer, 0, n)
                                digest.update(buffer, 0, n)
                                done += n
                                val percent = if (total > 0) (done * 100 / total).toInt() else 0
                                if (percent != shown) {
                                    shown = percent
                                    withContext(Dispatchers.Main) {
                                        state = UpdateState.Downloading(release, percent)
                                    }
                                }
                            }
                        }
                    }
                    val sha = digest.digest().joinToString("") { "%02x".format(it) }
                    if (sha != release.sha256) {
                        apk.delete()
                        error(context.getString(R.string.update_corrupted))
                    }
                    apk
                }
            }
            result.fold(
                onSuccess = { apk ->
                    state = UpdateState.Ready(release, apk)
                    launchInstaller(apk)
                },
                onFailure = {
                    Log.w(TAG, "update download failed", it)
                    state = UpdateState.Available(release)
                    onMessage(context.getString(R.string.update_failed, it.message))
                },
            )
        }
    }

    /** Android asks once to allow installing apps from this browser. */
    private fun launchInstaller(apk: File) {
        val pm = context.packageManager
        if (!pm.canRequestPackageInstalls()) {
            onMessage(context.getString(R.string.update_allow_installs))
            val settings = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            runCatching { context.startActivity(settings) }
            return
        }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", apk)
        val install = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(install)
        } catch (e: ActivityNotFoundException) {
            onMessage(context.getString(R.string.update_no_installer))
        }
    }

    private fun open(url: String): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 15_000
            readTimeout = 30_000
            setRequestProperty("User-Agent", "browsingUnicorns/$currentVersion (Android)")
            if (responseCode !in 200..299) error(context.getString(R.string.http_status_for, responseCode, url))
        }

    private fun get(url: String) = open(url).inputStream

    companion object {
        private const val TAG = "AppUpdates"
        private const val DAY_MS = 24L * 60 * 60 * 1000
        const val MANIFEST_URL = "https://ajustor.github.io/browsingUnicorns/latest.json"
        const val APK_NAME = "browsingUnicorns-android.apk"

        /** `0.10.0` > `0.9.3`; a pre-release suffix (`-rc.1`) is ignored. */
        fun isNewer(candidate: String, current: String): Boolean {
            fun parts(v: String) = v.trim().removePrefix("v").substringBefore('-')
                .split('.').map { it.toIntOrNull() ?: 0 }
            val a = parts(candidate)
            val b = parts(current)
            for (i in 0 until maxOf(a.size, b.size)) {
                val x = a.getOrElse(i) { 0 }
                val y = b.getOrElse(i) { 0 }
                if (x != y) return x > y
            }
            return false
        }
    }
}
