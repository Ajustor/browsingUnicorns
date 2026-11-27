# R8 rules of the release build (the libraries bring their own).

# UniFFI's bindings reach the Rust library through JNA, which finds its
# native methods, structures' fields and callbacks by reflection (and from
# its JNI code): neither may be renamed or removed.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class uniffi.mobile.** { *; }
-dontwarn java.awt.**
-dontwarn com.sun.jna.**
