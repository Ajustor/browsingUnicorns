//! Generates the Kotlin bindings from the built library:
//!
//! ```sh
//! cargo run -p mobile --features cli --bin uniffi-bindgen -- generate \
//!   --library target/aarch64-linux-android/release/libmobile.so \
//!   --language kotlin --out-dir android/app/src/main/java
//! ```

fn main() {
    uniffi::uniffi_bindgen_main()
}
