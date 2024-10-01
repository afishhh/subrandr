fn main() {
    println!("cargo:rustc-link-lib=freetype");
    println!("cargo:rustc-link-lib=harfbuzz");
}
