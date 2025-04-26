use std::{path::PathBuf, process::Command};

fn main() {
    if let Ok(rev) = std::env::var("SUBRANDR_BUILD_REV") {
        println!("cargo:rustc-env=BUILD_REV={}", &rev[..7]);
        println!("cargo:rustc-env=BUILD_DIRTY=");
    } else {
        let rev_output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .unwrap();
        let rev = String::from_utf8(rev_output.stdout).unwrap();
        let dirty_status = Command::new("git")
            .arg("diff-index")
            .arg("--quiet")
            .arg("HEAD")
            .status()
            .unwrap();
        let is_dirty = !dirty_status.success();

        println!("cargo:rustc-env=BUILD_REV={}", &rev[..7]);
        println!(
            "cargo:rustc-env=BUILD_DIRTY={}",
            if is_dirty { " (dirty)" } else { "" }
        );
    }

    let abiver = {
        let manifest_content =
            std::fs::read_to_string(std::env::var_os("CARGO_MANIFEST_PATH").unwrap()).unwrap();

        let mut result = None;
        for line in manifest_content.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };

            if name.trim() == "abiver" {
                if let Some(content) = value
                    .trim()
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                {
                    result = Some(content.to_owned());
                    break;
                }
            }
        }

        result
    }
    .unwrap();

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap();
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap();

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // cargo in its infinite wisdom refuses to provide you the target directory in build scripts
    let target_dir = PathBuf::from_iter(
        out_dir
            .components()
            .take_while(|x| x.as_os_str().to_str().is_none_or(|s| s != "build")),
    );

    macro_rules! cdylib_link_arg {
        ($fmt: literal $($rest: tt)*) => {
            println!(concat!("cargo:rustc-cdylib-link-arg=", $fmt) $($rest)*)
        };
    }

    match (&*os, &*env) {
        ("linux", _) => {
            cdylib_link_arg!("-Wl,-soname,libsubrandr.so.{}", abiver);
        }
        ("macos", _) | ("ios", _) => {
            // Searching "darwin linker man page" is the easiest way to find information about these.
            cdylib_link_arg!("-Wl,-compatibility_version,{}", abiver);
            cdylib_link_arg!("-Wl,-current_version,{}.{}.{}", major, minor, patch);
        }
        ("windows", "gnu") => {
            // The Windows approach is, as usual, the most arcane.
            // Apparently on Windows this is solved with stub "import libraries".
            // BUT there is no way we can tell the linker at this point that
            // "I want this import library to link to subrandr-1.dll instead of
            // subrandr.dll", because why would -soname work? Then it would make sense,
            // and we don't want that.
            // So instead the approach is as follows:
            // - Generate a .def file which contains all exported and imported
            //   symbols in a format that can be processed by `dlltool` to create
            //   an import library.
            // - Generate an import library at install time, with the correct ABI
            //   versioned dllname using the `implib` crate.
            //   (finding a working `dlltool` for the target seems very fragile)
            // Alternatively it would be nice if we could convince cargo to generate a
            // "subrandr-1.dll" instead so that it could ""just"" work.
            cdylib_link_arg!("-Wl,--output-def,{}/subrandr.def", target_dir.display());
        }
        _ => (),
    }
}
