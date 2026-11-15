<#
.SYNOPSIS
    Build the Android app on Windows: the Rust core for arm64 and x86_64
    (cargo ndk), its Kotlin bindings (uniffi-bindgen), then the APK.

.EXAMPLE
    .\scripts\build-android.ps1                 # debug APK
    .\scripts\build-android.ps1 assembleRelease
    .\scripts\build-android.ps1 installDebug    # onto the connected phone

    Needs the Android SDK and NDK (ANDROID_HOME, ANDROID_NDK_HOME), a JDK 17+
    (JAVA_HOME), `rustup target add aarch64-linux-android x86_64-linux-android`
    and `cargo install cargo-ndk`.
#>
param([string]$Task = 'assembleDebug')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o android\app\src\main\jniLibs build --release -p mobile
if ($LASTEXITCODE -ne 0) { throw 'cargo ndk failed' }

# The bindings are read from a host build: the release Android libraries are
# stripped of the metadata uniffi-bindgen needs.
cargo build -p mobile
if ($LASTEXITCODE -ne 0) { throw 'host build failed' }
cargo run -q -p mobile --features cli --bin uniffi-bindgen -- generate `
    --library target\debug\mobile.dll --language kotlin --no-format `
    --out-dir android\app\src\main\java
if ($LASTEXITCODE -ne 0) { throw 'uniffi-bindgen failed' }

Push-Location android
try {
    .\gradlew.bat --no-daemon $Task
    if ($LASTEXITCODE -ne 0) { throw "gradle $Task failed" }
    Get-ChildItem app\build\outputs\apk -Recurse -Filter *.apk | ForEach-Object { "{0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
} finally {
    Pop-Location
}
