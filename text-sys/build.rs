fn main() {
    println!("cargo:rustc-link-lib=freetype");
    println!("cargo:rustc-link-lib=harfbuzz");
    // NOTE: fontconfig is linked to in subrandr's `build.rs` so that it can
    //       be made conditional on its default platform feature logic.

    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "wasm32"
        && std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "wasi"
    {
        println!("cargo:rustc-link-lib=setjmp");
    }
}
