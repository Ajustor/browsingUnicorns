package io.github.ajustor.browsingunicorns

import org.junit.Assert.assertEquals
import org.junit.Test

class DesktopUserAgentTest {
    @Test
    fun theWebViewsChromeVersionIsKeptOnAComputer() {
        val mobile = "Mozilla/5.0 (Linux; Android 15; Pixel 8 Build/AP3A; wv) AppleWebKit/537.36 " +
            "(KHTML, like Gecko) Version/4.0 Chrome/142.0.7444.102 Mobile Safari/537.36"
        assertEquals(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
            desktopUserAgent(mobile),
        )
    }

    @Test
    fun withoutAChromeVersionARecentOneIsGiven() {
        assertEquals(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            desktopUserAgent("Mozilla/5.0 (Linux; Android 15)"),
        )
    }
}
