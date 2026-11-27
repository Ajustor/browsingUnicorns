package io.github.ajustor.browsingunicorns

import android.Manifest
import android.app.DownloadManager
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.util.Base64
import android.util.Log
import android.webkit.CookieManager
import android.webkit.JavascriptInterface
import android.webkit.URLUtil
import android.webkit.WebView
import androidx.core.content.ContextCompat
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import org.json.JSONObject

/**
 * The pages' downloads, into the shared Downloads folder: http(s) through
 * Android's DownloadManager (its notification, cookies and referrer kept),
 * `data:` decoded here, `blob:` read by the page itself and handed over.
 * [context] follows the activity shown (BrowserCore).
 */
class Downloads(private val context: Context) {
    /** Short messages for the user (started, saved, failed). */
    var onMessage: (String) -> Unit = {}

    private val main = Handler(Looper.getMainLooper())

    /** Blob downloads asked for, by one-time token: a page can't write files on its own. */
    private val pending = ConcurrentHashMap<String, Pending>()

    private class Pending(val name: String, val mimeType: String?)

    /**
     * Android 9 and older: writing to the Downloads folder needs the storage
     * permission; asks for it, then tells whether it was granted. The
     * activity sets it.
     */
    var askStorage: ((Boolean) -> Unit) -> Unit = { it(false) }

    /** The bridge a tab's page uses to hand over a blob's contents. */
    inner class Bridge {
        @JavascriptInterface
        fun save(token: String, dataUrl: String) {
            val p = pending.remove(token) ?: return
            runCatching { write(p.name, p.mimeType, decodeDataUrl(dataUrl)) }
                .onSuccess { tell(context.getString(R.string.download_saved, it)) }
                .onFailure { fail(p.name, it) }
        }

        @JavascriptInterface
        fun failed(token: String, reason: String) {
            val p = pending.remove(token) ?: return
            tell(context.getString(R.string.download_failed, p.name, reason))
        }
    }

    /** WebView's DownloadListener: [page] is the address of the page that asked. */
    fun start(view: WebView, url: String, userAgent: String?, contentDisposition: String?, mimeType: String?, page: String?) {
        val name = URLUtil.guessFileName(url, contentDisposition, mimeType)
        withStorage {
            when (Uri.parse(url).scheme?.lowercase()) {
                "http", "https" -> enqueue(url, name, userAgent, mimeType, page)
                "data" -> Thread {
                    runCatching { write(name, mimeType, decodeDataUrl(url)) }
                        .onSuccess { tell(context.getString(R.string.download_saved, it)) }
                        .onFailure { fail(name, it) }
                }.start()
                "blob" -> fromBlob(view, url, name, mimeType)
                else -> onMessage(context.getString(R.string.download_unsupported, url))
            }
        }
    }

    private fun enqueue(url: String, name: String, userAgent: String?, mimeType: String?, page: String?) {
        try {
            val request = DownloadManager.Request(Uri.parse(url)).apply {
                CookieManager.getInstance().getCookie(url)?.let { addRequestHeader("Cookie", it) }
                userAgent?.let { addRequestHeader("User-Agent", it) }
                page?.let { addRequestHeader("Referer", it) }
                mimeType?.takeIf { it.isNotBlank() }?.let { setMimeType(it) }
                setTitle(name)
                setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
            }
            context.getSystemService(DownloadManager::class.java).enqueue(request)
            onMessage(context.getString(R.string.downloading, name))
        } catch (e: Exception) {
            fail(name, e)
        }
    }

    /** A `blob:` address only lives in its page: the page reads it and sends it back. */
    private fun fromBlob(view: WebView, url: String, name: String, mimeType: String?) {
        val token = UUID.randomUUID().toString()
        pending[token] = Pending(name, mimeType)
        val u = JSONObject.quote(url)
        val t = JSONObject.quote(token)
        view.evaluateJavascript(
            """
            (async () => {
              try {
                const blob = await (await fetch($u)).blob();
                const reader = new FileReader();
                reader.onload = () => BUDownloads.save($t, reader.result);
                reader.onerror = () => BUDownloads.failed($t, String(reader.error));
                reader.readAsDataURL(blob);
              } catch (e) {
                BUDownloads.failed($t, String(e));
              }
            })();
            """.trimIndent(),
            null,
        )
        onMessage(context.getString(R.string.downloading, name))
    }

    private fun withStorage(then: () -> Unit) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            then()
        } else {
            askStorage { granted ->
                if (granted) then() else onMessage(context.getString(R.string.download_permission_denied))
            }
        }
    }

    /** Writes [bytes] in the Downloads folder; returns the name it got. */
    private fun write(name: String, mimeType: String?, bytes: ByteArray): String {
        val type = mimeType?.takeIf { it.isNotBlank() } ?: "application/octet-stream"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val resolver = context.contentResolver
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.MIME_TYPE, type)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: error(context.getString(R.string.download_no_room))
            try {
                resolver.openOutputStream(uri)!!.use { it.write(bytes) }
                resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            } catch (e: Exception) {
                resolver.delete(uri, null, null)
                throw e
            }
            return name
        }
        @Suppress("DEPRECATION")
        val dir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).apply { mkdirs() }
        val file = unique(dir, name)
        file.writeBytes(bytes)
        MediaScannerConnection.scanFile(context, arrayOf(file.absolutePath), arrayOf(type), null)
        return file.name
    }

    /** `name`, or `name (1)`, `name (2)`... if taken. */
    private fun unique(dir: File, name: String): File {
        val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
        val base = name.substring(0, dot)
        val ext = name.substring(dot)
        var file = File(dir, name)
        var i = 1
        while (file.exists()) file = File(dir, "$base (${i++})$ext")
        return file
    }

    private fun tell(text: String) = main.post { onMessage(text) }

    private fun fail(name: String, e: Throwable) {
        Log.w(TAG, "download of $name failed", e)
        tell(context.getString(R.string.download_failed, name, e.message ?: e.javaClass.simpleName))
    }

    companion object {
        private const val TAG = "Downloads"

        /** The bytes of a `data:[<type>][;base64],<data>` address. */
        fun decodeDataUrl(url: String): ByteArray {
            val comma = url.indexOf(',')
            require(url.startsWith("data:", ignoreCase = true) && comma > 0) { "not a data: address" }
            val meta = url.substring(5, comma)
            val data = url.substring(comma + 1)
            return if (meta.endsWith(";base64", ignoreCase = true)) {
                Base64.decode(data, Base64.DEFAULT)
            } else {
                Uri.decode(data).toByteArray()
            }
        }
    }
}
