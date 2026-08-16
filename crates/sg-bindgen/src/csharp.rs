//! Generates the C# P/Invoke declarations for the Windows app.
//!
//! UniFFI has no C# backend and `uniffi-bindgen-cs` is pinned to uniffi 0.31 while this workspace
//! is on 0.32, so `crates/sg-ffi/src/cabi.rs` is a hand-written `extern "C"` surface. This reads
//! those signatures and writes the matching `[DllImport]`s, so the declarations on the C# side are
//! generated from the Rust rather than transcribed by hand — which is the half of the fallback
//! that keeps the two from drifting apart silently.
//!
//!     cargo run -p sg-bindgen --bin csharp-bindgen              # write the committed file
//!     cargo run -p sg-bindgen --bin csharp-bindgen -- <path>    # write somewhere else
//!
//! The output is committed, exactly as the generated Swift bindings under
//! `apps/shared/ServerGlassFFI/generated` are, so a checkout builds without running a generator
//! first. `scripts/build-windows.ps1` regenerates it and CI checks it is current.
//!
//! The optional path exists for that check: it generates to a temporary file and compares, so a
//! verification run never leaves the tree dirty — a check that has to modify what it is checking
//! is one nobody can run on a release branch.

use std::path::PathBuf;

fn main() {
    // Relative to the crate, so the command works from anywhere in the tree.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the workspace root is two levels above this crate")
        .to_path_buf();

    let input = root.join("crates/sg-ffi/src/cabi.rs");
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("apps/windows/ServerGlass.Core/Generated/NativeMethods.g.cs"));

    std::fs::create_dir_all(output.parent().expect("the output has a directory"))
        .expect("could not create the output directory");

    csbindgen::Builder::default()
        .input_extern_file(&input)
        // The cdylib is `sg_ffi.dll` on Windows. `ServerGlassCore` resolves it from beside the
        // executable, which is where the build script puts it.
        .csharp_dll_name("sg_ffi")
        .csharp_namespace("ServerGlass.Core")
        .csharp_class_name("Native")
        // Internal: nothing outside ServerGlass.Core may touch a raw pointer, and the compiler
        // should be the thing enforcing that rather than a convention.
        .csharp_class_accessibility("internal")
        .csharp_use_nint_types(false)
        .generate_csharp_file(&output)
        .expect("could not generate the C# bindings");

    println!("wrote {}", output.display());
}
