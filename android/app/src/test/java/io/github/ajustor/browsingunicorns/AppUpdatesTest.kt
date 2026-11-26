package io.github.ajustor.browsingunicorns

import org.junit.Assert.assertEquals
import org.junit.Test

class AppUpdatesTest {
    @Test
    fun aPhoneLooksForItsOwnApkFirst() {
        assertEquals(
            listOf("browsingUnicorns-android-arm64.apk", "browsingUnicorns-android.apk"),
            AppUpdates.apkNames(listOf("arm64-v8a", "armeabi-v7a", "armeabi")),
        )
        assertEquals(
            listOf("browsingUnicorns-android-x86_64.apk", "browsingUnicorns-android.apk"),
            AppUpdates.apkNames(listOf("x86_64")),
        )
        // A processor without its own APK: the universal one.
        assertEquals(listOf("browsingUnicorns-android.apk"), AppUpdates.apkNames(listOf("armeabi-v7a")))
    }

    @Test
    fun versionsCompareByNumbers() {
        assertEquals(true, AppUpdates.isNewer("0.31.0", "0.30.0"))
        assertEquals(false, AppUpdates.isNewer("0.30.0", "0.30.0"))
    }
}
