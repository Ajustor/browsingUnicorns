plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The desktop app's version (Cargo.toml), so both release together.
val appVersion: String = rootProject.file("../Cargo.toml").readLines()
    .first { it.startsWith("version = ") }
    .substringAfter('"').substringBefore('"')

android {
    namespace = "io.github.ajustor.browsingunicorns"
    compileSdk = 37

    defaultConfig {
        applicationId = "io.github.ajustor.browsingunicorns"
        minSdk = 26
        targetSdk = 36
        versionName = appVersion
        // 0.1.0 -> 100: grows with every release.
        versionCode = appVersion.substringBefore('-').split('.')
            .map(String::toInt).let { (a, b, c) -> a * 10_000 + b * 100 + c }
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        // `-PsyncRelay=http://10.0.2.2:8787` points the app at a local relay.
        buildConfigField(
            "String",
            "SYNC_RELAY",
            "\"${project.findProperty("syncRelay") ?: ""}\"",
        )
    }

    signingConfigs {
        // CI signs releases with the keystore given in the environment; a
        // local build without it is signed with the debug key.
        val keystore = System.getenv("ANDROID_KEYSTORE")
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    // Page scripts injected at document start (WebViewCompat.addDocumentStartJavaScript).
    implementation("androidx.webkit:webkit:1.13.0")
    // UniFFI's Kotlin bindings call the Rust library through JNA.
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    // Pairing: the code as a QR code, and scanning one (no camera permission:
    // Google's scanner UI runs in Play services).
    implementation("com.google.zxing:core:3.5.4")
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
}
