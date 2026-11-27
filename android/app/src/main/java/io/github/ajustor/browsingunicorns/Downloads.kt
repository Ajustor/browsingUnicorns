package io.github.ajustor.browsingunicorns

import android.Manifest
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
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
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.io.File
import java.io.OutputStream
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import org.json.JSONObject

/** A download saved in the Downloads folder, that the user may open. */
class DownloadDone(val name: String, val uri: Uri?, val mimeType: String?)

/**
 * The pages' downloads, into the shared Downloads folder: http(s) through
 * Android's DownloadManager (its notification, cookies and referrer kept),
 * `data:` decoded here, `blob:` read by the page itself and handed over.
 * [context] follows the activity shown (BrowserCore).
 */
class Downloads(private val context: Context) {
    /** Short messages for the user (started, failed). */
    var onMessage: (String) -> Unit = {}

    /** A download finished, saved (on the main thread). */
    var onDone: (DownloadDone) -> Unit = {}

    /** The DownloadManager downloads started here and not finished yet. */
    private val running = ConcurrentHashMap.newKeySet<Long>()

    /** Hears DownloadManager's downloads finish, while some of [running] run. */
    private var receiver: BroadcastReceiver? = null

    private val main = Handler(Looper.getMainLooper())

    /** Blob downloads asked for, by one-time token: a page can't write files on its own. */
    private val pending = ConcurrentHashMap<String, Pending>()

    private class Pending(val name: String, val mimeType: String?) {
        /** Where its chunks go, from the first one. */
        var sink: Sink? = null
    }

    /**
     * Android 9 and older: writing to the Downloads folder needs the storage
     * permission; asks for it, then tells whether it was granted. The
     * activity sets it.
     */
    var askStorage: ((Boolean) -> Unit) -> Unit = { it(false) }

    /**
     * The bridge a tab's page uses to hand over a blob's contents, in chunks
     * (one whole would take the blob's size several times over in memory).
     */
    inner class Bridge {
        /** The next chunk of the blob of [token], in base64; [last] ends it. */
        @JavascriptInterface
        fun save(token: String, chunk: String, last: Boolean) {
            val p = pending[token] ?: return
            synchronized(p) {
                try {
                    val sink = p.sink ?: openSink(p.name, p.mimeType).also { p.sink = it }
                    sink.out.write(Base64.decode(chunk, Base64.DEFAULT))
                    if (last) {
                        pending.remove(token)
                        sink.finish()
                    }
                } catch (e: Exception) {
                    pending.remove(token)
                    p.sink?.abort()
                    fail(p.name, e)
                }
            }
        }

        @JavascriptInterface
        fun failed(token: String, reason: String) {
            val p = pending.remove(token) ?: return
            synchronized(p) { p.sink?.abort() }
            tell(context.getString(R.string.download_failed, p.name, reason))
        }
    }

    /** WebView's DownloadListener: [page] is the address of the page that asked. */
    fun start(view: WebView, url: String, userAgent: String?, contentDisposition: String?, mimeType: String?, page: String?) {
        val name = URLUtil.guessFileName(url, contentDisposition, mimeType)
        withStorage {
            when (Uri.parse(url).scheme?.lowercase()) {
                "http", "https" -> enqueue(url, name, userAgent, mimeType, page, cookies(view, url))
                "data" -> Thread {
                    runCatching {
                        val sink = openSink(name, mimeType)
                        try {
                            sink.out.write(decodeDataUrl(url))
                            sink.finish()
                        } catch (e: Exception) {
                            sink.abort()
                            throw e
                        }
                    }.onFailure { fail(name, it) }
                }.start()
                "blob" -> fromBlob(view, url, name, mimeType)
                else -> onMessage(context.getString(R.string.download_unsupported, url))
            }
        }
    }

    /**
     * [view]'s cookies for [url]: those of its profile, a private tab's own
     * (not the normal tabs', which would tell who the user is).
     */
    private fun cookies(view: WebView, url: String): String? = runCatching {
        val manager = if (WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
            WebViewCompat.getProfile(view).cookieManager
        } else {
            CookieManager.getInstance()
        }
        manager.getCookie(url)
    }.getOrNull()

    private fun enqueue(url: String, name: String, userAgent: String?, mimeType: String?, page: String?, cookies: String?) {
        try {
            val request = DownloadManager.Request(Uri.parse(url)).apply {
                cookies?.let { addRequestHeader("Cookie", it) }
                userAgent?.let { addRequestHeader("User-Agent", it) }
                page?.let { addRequestHeader("Referer", it) }
                mimeType?.takeIf { it.isNotBlank() }?.let { setMimeType(it) }
                setTitle(name)
                setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
            }
            running += context.getSystemService(DownloadManager::class.java).enqueue(request)
            listen()
            onMessage(context.getString(R.string.downloading, name))
        } catch (e: Exception) {
            fail(name, e)
        }
    }

    /** A `blob:` address only lives in its page: the page reads it and sends it back, by slices. */
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
                const read = (part) => new Promise((done, fail) => {
                  const reader = new FileReader();
                  reader.onload = () => done(reader.result);
                  reader.onerror = () => fail(reader.error);
                  reader.readAsDataURL(part);
                });
                let at = 0;
                do {
                  const url = await read(blob.slice(at, at + $CHUNK));
                  at += $CHUNK;
                  const comma = url.indexOf(',');
                  BUDownloads.save($t, comma < 0 ? '' : url.slice(comma + 1), at >= blob.size);
                } while (at < blob.size);
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

    /** A file being written in the Downloads folder. */
    private interface Sink {
        val out: OutputStream

        /** Closes it, now complete, and tells (onDone). */
        fun finish()

        /** Closes and deletes it. */
        fun abort()
    }

    /** A new file [name] in the Downloads folder, to write into. */
    private fun openSink(name: String, mimeType: String?): Sink {
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
            val out = try {
                resolver.openOutputStream(uri) ?: error(context.getString(R.string.download_no_room))
            } catch (e: Exception) {
                resolver.delete(uri, null, null)
                throw e
            }
            return object : Sink {
                override val out = out.buffered()

                override fun finish() {
                    this.out.close()
                    resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
                    done(DownloadDone(name, uri, type))
                }

                override fun abort() {
                    runCatching { this.out.close() }
                    runCatching { resolver.delete(uri, null, null) }
                }
            }
        }
        @Suppress("DEPRECATION")
        val dir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).apply { mkdirs() }
        val file = unique(dir, name)
        return object : Sink {
            override val out = file.outputStream().buffered()

            override fun finish() {
                out.close()
                // Opened through the media store's address (a file:// one can't be shared).
                MediaScannerConnection.scanFile(context, arrayOf(file.absolutePath), arrayOf(type)) { _, uri ->
                    done(DownloadDone(file.name, uri, type))
                }
            }

            override fun abort() {
                runCatching { out.close() }
                file.delete()
            }
        }
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

    private fun done(download: DownloadDone) = main.post { onDone(download) }

    /** Listens for DownloadManager's downloads finishing (a system broadcast: exported). */
    private fun listen() {
        if (receiver != null) return
        receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, intent: Intent) {
                val id = intent.getLongExtra(DownloadManager.EXTRA_DOWNLOAD_ID, -1)
                if (!running.remove(id)) return
                finished(id)
                if (running.isEmpty()) close()
            }
        }.also {
            ContextCompat.registerReceiver(
                context.applicationContext,
                it,
                IntentFilter(DownloadManager.ACTION_DOWNLOAD_COMPLETE),
                ContextCompat.RECEIVER_EXPORTED,
            )
        }
    }

    /** DownloadManager's download [id] ended: saved, it is offered to open (a failure has its notification). */
    private fun finished(id: Long) {
        val manager = context.getSystemService(DownloadManager::class.java)
        val (status, title) = runCatching {
            manager.query(DownloadManager.Query().setFilterById(id))?.use { c ->
                if (!c.moveToFirst()) return@use null
                c.getInt(c.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS)) to
                    c.getString(c.getColumnIndexOrThrow(DownloadManager.COLUMN_TITLE)).orEmpty()
            }
        }.getOrNull() ?: return
        if (status != DownloadManager.STATUS_SUCCESSFUL) return
        val uri = runCatching { manager.getUriForDownloadedFile(id) }.getOrNull()
        val type = runCatching { manager.getMimeTypeForDownloadedFile(id) }.getOrNull()
        onDone(DownloadDone(title, uri, type))
    }

    /** Stops listening for DownloadManager (the browser ends, or nothing runs). */
    fun close() {
        receiver?.let { runCatching { context.applicationContext.unregisterReceiver(it) } }
        receiver = null
    }

    /**
     * Opens [download] in the app that shows its type; without one, the
     * system's list of downloads.
     */
    fun open(download: DownloadDone) {
        val uri = download.uri
        if (uri != null) {
            val type = download.mimeType?.takeIf { it.isNotBlank() } ?: context.contentResolver.getType(uri)
            val view = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, type)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            try {
                return context.startActivity(view)
            } catch (_: ActivityNotFoundException) {
            } catch (e: SecurityException) {
                Log.w(TAG, "opening ${download.name} refused", e)
            }
        }
        try {
            context.startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS))
        } catch (_: ActivityNotFoundException) {
            onMessage(context.getString(R.string.download_no_viewer, download.name))
        }
    }

    private fun fail(name: String, e: Throwable) {
        Log.w(TAG, "download of $name failed", e)
        tell(context.getString(R.string.download_failed, name, e.message ?: e.javaClass.simpleName))
    }

    companion object {
        private const val TAG = "Downloads"

        /** The size of the slices a blob is handed over by (bytes). */
        private const val CHUNK = 512 * 1024

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
