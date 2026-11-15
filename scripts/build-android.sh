#!/usr/bin/env bash
# Build the Android app: the Rust core for arm64 and x86_64 (cargo ndk), its
# Kotlin bindings (uniffi-bindgen), then the APK with Gradle.
#
# Usage: scripts/build-android.sh [gradle task]   (default: assembleDebug)
#
# Needs: the Android SDK and NDK (ANDROID_HOME, ANDROID_NDK_HOME), JDK 17+,
# `rustup target add aarch64-linux-android x86_64-linux-android` and
# `cargo install cargo-ndk`.
set -euo pipefail

task="${1:-assembleDebug}"

# On GitHub runners the NDK is announced by ANDROID_NDK_LATEST_HOME /
# ANDROID_NDK_ROOT; an empty ANDROID_NDK_HOME would hide them from cargo-ndk.
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_ROOT:-}}}"
[ -n "$ANDROID_NDK_HOME" ] || unset ANDROID_NDK_HOME
cd "$(dirname "$0")/.."

cargo ndk -t arm64-v8a -t x86_64 --platform 26 \
  -o android/app/src/main/jniLibs build --release -p mobile

# The bindings are read from a host build: the release Android libraries are
# stripped of the metadata uniffi-bindgen needs.
cargo build -p mobile
case "$(uname -s)" in
  Darwin) host_lib=target/debug/libmobile.dylib ;;
  MINGW*|MSYS*|CYGWIN*) host_lib=target/debug/mobile.dll ;;
  *) host_lib=target/debug/libmobile.so ;;
esac
cargo run -q -p mobile --features cli --bin uniffi-bindgen -- generate \
  --library "$host_lib" --language kotlin --no-format \
  --out-dir android/app/src/main/java

cd android
./gradlew --no-daemon "$task"
find app/build/outputs/apk -name '*.apk' -exec ls -l {} \;
