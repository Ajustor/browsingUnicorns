package io.github.ajustor.browsingunicorns

import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.util.Log
import android.webkit.CookieManager
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import uniffi.mobile.CookieInfo

/**
 * The normal tabs' cookies, for sync (signed in on one device, signed in on
 * the others). Android's CookieManager can't list them: they are read from
 * a copy of the WebView's cookie database (Chromium's format, its values
 * not encrypted on Android), and written back through the CookieManager.
 * Called from the sync thread.
 */
class SiteCookies(private val context: Context) {

    /** Every cookie of the normal tabs; `null` when the database can't be read. */
    fun all(): List<CookieInfo>? {
        val source = database() ?: return null
        val copy = File(context.cacheDir, "cookies-for-sync.db")
        return try {
            // Everything on disk first; the WebView may hold the database locked.
            CookieManager.getInstance().flush()
            source.copyTo(copy, overwrite = true)
            SQLiteDatabase.openDatabase(copy.path, null, SQLiteDatabase.OPEN_READONLY).use { db ->
                db.rawQuery("SELECT * FROM cookies", null).use(::read)
            }
        } catch (e: Exception) {
            Log.w(TAG, "reading the cookies failed", e)
            null
        } finally {
            copy.delete()
            File(copy.path + "-journal").delete()
        }
    }

    /** Sets [cookies] in the normal tabs' cookie store. */
    fun set(cookies: List<CookieInfo>) {
        val manager = CookieManager.getInstance()
        for (c in cookies) {
            val host = c.domain.trimStart('.')
            if (host.isEmpty()) continue
            val url = (if (c.secure) "https://" else "http://") + host + c.path
            manager.setCookie(url, header(c))
        }
        manager.flush()
    }

    /** The database of the default WebView profile (its place changed over versions). */
    private fun database(): File? {
        val root = File(context.applicationInfo.dataDir, "app_webview")
        return listOf("Default/Cookies", "Cookies").map { File(root, it) }.firstOrNull { it.isFile }
    }

    private fun read(cursor: Cursor): List<CookieInfo> {
        fun column(name: String) = cursor.getColumnIndex(name)
        val host = column("host_key")
        val name = column("name")
        val value = column("value")
        val path = column("path")
        val expires = column("expires_utc")
        val secure = column("is_secure").takeIf { it >= 0 } ?: column("secure")
        val httpOnly = column("is_httponly").takeIf { it >= 0 } ?: column("httponly")
        val sameSite = column("samesite")
        val persistent = column("is_persistent").takeIf { it >= 0 } ?: column("persistent")
        if (host < 0 || name < 0 || value < 0) return emptyList()
        val out = ArrayList<CookieInfo>(cursor.count)
        while (cursor.moveToNext()) {
            val text = cursor.getString(value) ?: ""
            // An encrypted value (not on Android in practice) can't be shared.
            if (text.isEmpty()) continue
            val isPersistent = persistent < 0 || cursor.getInt(persistent) != 0
            val expiresUtc = if (expires >= 0) cursor.getLong(expires) else 0L
            out += CookieInfo(
                domain = cursor.getString(host) ?: continue,
                name = cursor.getString(name) ?: "",
                value = text,
                path = if (path >= 0) cursor.getString(path) ?: "/" else "/",
                secure = secure >= 0 && cursor.getInt(secure) != 0,
                httpOnly = httpOnly >= 0 && cursor.getInt(httpOnly) != 0,
                sameSite = when (if (sameSite >= 0) cursor.getInt(sameSite) else -1) {
                    0 -> "None"
                    1 -> "Lax"
                    2 -> "Strict"
                    else -> ""
                },
                expires = if (isPersistent && expiresUtc > 0) expiresUtc / 1_000_000 - WINDOWS_TO_UNIX else null,
            )
        }
        return out
    }

    /** [c] as a `Set-Cookie` header for CookieManager.setCookie. */
    private fun header(c: CookieInfo): String = buildString {
        append(c.name).append('=').append(c.value)
        // A host-only cookie has no Domain attribute.
        if (c.domain.startsWith('.')) append("; Domain=").append(c.domain)
        append("; Path=").append(c.path.ifEmpty { "/" })
        c.expires?.let { append("; Expires=").append(httpDate(it)) }
        if (c.secure) append("; Secure")
        if (c.httpOnly) append("; HttpOnly")
        if (c.sameSite.isNotEmpty()) append("; SameSite=").append(c.sameSite)
    }

    private fun httpDate(seconds: Long): String =
        SimpleDateFormat("EEE, dd MMM yyyy HH:mm:ss 'GMT'", Locale.US)
            .apply { timeZone = TimeZone.getTimeZone("GMT") }
            .format(Date(seconds * 1000))

    companion object {
        private const val TAG = "SiteCookies"

        /** Seconds from 1601-01-01 (Chromium's epoch) to 1970-01-01. */
        private const val WINDOWS_TO_UNIX = 11_644_473_600L
    }
}
