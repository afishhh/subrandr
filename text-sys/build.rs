fn main() {
    println!("cargo:rustc-link-lib=freetype");
    println!("cargo:rustc-link-lib=harfbuzz");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap() == "unix" {
        println!("cargo:rustc-link-lib=fontconfig");
    }
}
