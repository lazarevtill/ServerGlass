//! Generates the Swift / Kotlin / C# bindings for `sg-ffi`.
//!
//! Lives in its own crate because the generator needs UniFFI's `cli` feature, which pulls in clap
//! and the whole bindgen backend; enabling it on `sg-ffi` itself would drag all of that into the
//! static library that ships inside the app.
//!
//!     cargo run -p sg-bindgen -- generate --library target/debug/libsg_ffi.dylib \
//!         --language swift --out-dir apps/macos/Sources/ServerGlassCore/generated
fn main() {
    uniffi::uniffi_bindgen_main()
}
