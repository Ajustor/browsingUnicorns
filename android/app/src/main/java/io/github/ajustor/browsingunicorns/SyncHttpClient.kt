package io.github.ajustor.browsingunicorns

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject
import uniffi.mobile.MobileException
import uniffi.mobile.SyncHttp

/** HTTP to the sync relay for the Rust core. Blocking: called off the main thread. */
class SyncHttpClient : SyncHttp {
    override fun put(url: String, token: String, body: ByteArray) {
        request("PUT", url, token, body)
    }

    override fun get(url: String, token: String): String = request("GET", url, token, null)

    override fun delete(url: String, token: String) {
        request("DELETE", url, token, null)
    }

    private fun request(method: String, url: String, token: String, body: ByteArray?): String {
        val connection = try {
            URL(url).openConnection() as HttpURLConnection
        } catch (e: IOException) {
            throw MobileException.Failed("relay unreachable: ${e.message}")
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
                throw MobileException.Failed("relay: $code $message")
            }
            return text
        } catch (e: IOException) {
            throw MobileException.Failed("relay unreachable: ${e.message}")
        } finally {
            connection.disconnect()
        }
    }
}
