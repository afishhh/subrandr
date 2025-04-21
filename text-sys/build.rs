fn main() {
    println!("cargo:rustc-link-lib=freetype");
    println!("cargo:rustc-link-lib=harfbuzz");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap() == "unix"
        || std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows"
    {
        println!("cargo:rustc-link-lib=fontconfig");
    }

    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "wasm32"
        && std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "wasi"
    {
        println!("cargo:rustc-link-lib=setjmp");
    }
}
