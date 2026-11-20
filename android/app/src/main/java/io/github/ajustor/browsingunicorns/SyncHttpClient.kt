package io.github.ajustor.browsingunicorns

import android.content.Context
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject
import uniffi.mobile.MobileException
import uniffi.mobile.SyncHttp

/**
 * HTTP for the Rust core: the sync relay, extension downloads. Blocking: called off the main thread.
 * Its errors reach the user inside the core's messages: [context] gives their texts.
 */
class SyncHttpClient(private val context: Context) : SyncHttp {
    override fun download(url: String): ByteArray {
        var address = URL(url)
        // HttpURLConnection does not follow redirects across hosts by itself.
        repeat(5) {
            val connection = try {
                address.openConnection() as HttpURLConnection
            } catch (e: IOException) {
                throw MobileException.Failed(context.getString(R.string.http_unreachable, e.message))
            }
            try {
                connection.instanceFollowRedirects = false
                connection.connectTimeout = 10_000
                connection.readTimeout = 60_000
                connection.setRequestProperty("User-Agent", "browsingUnicorns-android/${BuildConfig.VERSION_NAME}")
                val code = connection.responseCode
                if (code in 300..399) {
                    val location = connection.getHeaderField("Location")
                        ?: throw MobileException.Failed(context.getString(R.string.http_redirect_without_location))
                    address = URL(address, location)
                    return@repeat
                }
                if (code !in 200..299) throw MobileException.Failed(context.getString(R.string.http_status, code))
                return connection.inputStream.use { input ->
                    val out = java.io.ByteArrayOutputStream()
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        out.write(buffer, 0, n)
                        if (out.size() > MAX_DOWNLOAD) throw MobileException.Failed(context.getString(R.string.http_package_too_big))
                    }
                    out.toByteArray()
                }
            } catch (e: IOException) {
                throw MobileException.Failed(context.getString(R.string.http_download_failed, e.message))
            } finally {
                connection.disconnect()
            }
        }
        throw MobileException.Failed(context.getString(R.string.http_too_many_redirects))
    }

    override fun put(url: String, token: String, body: ByteArray) {
        request("PUT", url, token, body)
    }

    override fun get(url: String, token: String): String = request("GET", url, token, null)

    override fun delete(url: String, token: String) {
        request("DELETE", url, token, null)
    }

    private companion object {
        const val MAX_DOWNLOAD = 100 * 1024 * 1024
    }

    private fun request(method: String, url: String, token: String, body: ByteArray?): String {
        val connection = try {
            URL(url).openConnection() as HttpURLConnection
        } catch (e: IOException) {
            throw MobileException.Failed(context.getString(R.string.relay_unreachable, e.message))
        }
        try {
            connection.requestMethod = method
            connection.connectTimeout = 10_000
            connection.readTimeout = 30_000
            connection.setRequestProperty("Authorization", "Bearer $token")
            connection.setRequestProperty("User-Agent", "browsingUnicorns-android/${BuildConfig.VERSION_NAME}")
            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/octet-stream")
                connection.setFixedLengthStreamingMode(body.size)
                connection.outputStream.use { it.write(body) }
            }
            val code = connection.responseCode
            val stream = if (code in 200..299) connection.inputStream else connection.errorStream
            val text = stream?.bufferedReader()?.use { it.readText() } ?: ""
            if (code !in 200..299) {
                val message = runCatching { JSONObject(text).getString("error") }.getOrDefault(text)
                throw MobileException.Failed(context.getString(R.string.relay_error, code, message))
            }
            return text
        } catch (e: IOException) {
            throw MobileException.Failed(context.getString(R.string.relay_unreachable, e.message))
        } finally {
            connection.disconnect()
        }
    }
}
